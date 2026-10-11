// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Components declared while the engine runs, in a file.
//!
//! Such a component has no Rust type, so no registration knows its name: a
//! file holding one also holds its declaration — name, domain, fields — the
//! way a page describes itself. A world that never heard of it registers it
//! from the file; a world that declares it differently reads the file's
//! fields onto its own by name.
//!
//! A value is written field by field, by name, each field as the tagged
//! script value it is — `{"Int": 42}`, `{"Vec3": …}` — so a cell reads back
//! as what it was whatever declaration its reader has: records written under
//! two declarations of one component (a prefab saved before its scene, a game
//! saved before its scene was) merge, and every cell still says what it
//! holds. The file's declaration only registers the component where the
//! world has none; a cell is read onto the live declaration by name and by
//! value.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ErgonType, ScriptValue};

use super::component_registration::{registration_named, Staged, StagedComponent};
use super::record::{
    from_record, to_record, EntityRef, Record, RecordError, ReferenceReader, ReferenceWriter,
    ReportEntry, ReportKind,
};
use super::scene_record::PageRecord;
use crate::ecs::packed::PackedColumn;
use crate::ecs::{
    AnyVec, ComponentKey, ComponentProvenance, ComponentVTable, FieldKind, PackedField,
    PackedLayout, RuntimeComponentDecl, SemanticDomain, World,
};

/// A declared component, as a file describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredRecord {
    /// Its name.
    pub name: String,
    /// The domain whose pages hold it.
    pub domain: SemanticDomain,
    /// Its fields, in declaration order.
    pub fields: Vec<DeclaredFieldRecord>,
}

/// One field of a [`DeclaredRecord`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredFieldRecord {
    /// Its name.
    pub name: String,
    /// What it holds.
    pub kind: FieldKind,
    /// Its Ergon type, where its declaration gave one: what tells apart two
    /// values of one kind (a `string?` and an `int?`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<ErgonType>,
    /// What a new row starts with, written as a value of the field is.
    pub default: Record,
}

/// The declaration a file holds for `vtable`'s component, when it is one
/// declared while the engine runs.
pub(super) fn declaration_of(vtable: &ComponentVTable) -> Option<DeclaredRecord> {
    let layout = vtable.columns.packed()?;
    Some(DeclaredRecord {
        name: vtable.name.to_string(),
        domain: vtable.domain,
        fields: layout
            .fields()
            .iter()
            .enumerate()
            .map(|(slot, field)| DeclaredFieldRecord {
                name: field.name.clone(),
                kind: field.kind,
                ty: layout.ty(slot).cloned(),
                // An entity a default names is no entity of the file.
                default: encode(&field.default, &mut Outside).unwrap_or(Record::Unit),
            })
            .collect(),
    })
}

/// The declarations among `pool` that `pages` hold, sorted by name: what a
/// record built of those pages describes. Of two declarations of one name,
/// the later in `pool` is kept — the newer record's.
pub(crate) fn described<'a>(
    pages: &[PageRecord],
    pool: impl IntoIterator<Item = &'a DeclaredRecord>,
) -> Vec<DeclaredRecord> {
    let mut by_name: BTreeMap<&str, &DeclaredRecord> = BTreeMap::new();
    for declared in pool {
        by_name.insert(&declared.name, declared);
    }
    if by_name.is_empty() {
        return Vec::new();
    }
    let used: BTreeSet<&str> = pages
        .iter()
        .flat_map(|page| page.components.iter().map(String::as_str))
        .collect();
    by_name
        .into_iter()
        .filter(|(name, _)| used.contains(name))
        .map(|(_, declared)| declared.clone())
        .collect()
}

/// `declared` as one value, for a format that writes a record's parts
/// itself: `{ name, domain, fields: [{ name, kind, default }] }`.
pub(crate) fn declaration_to_record(declared: &DeclaredRecord) -> Record {
    let text = |value: &str| Record::Str(value.to_owned());
    Record::Struct {
        name: "Declared".to_owned(),
        fields: vec![
            ("name".to_owned(), text(&declared.name)),
            ("domain".to_owned(), text(declared.domain.name())),
            (
                "fields".to_owned(),
                Record::Seq(
                    declared
                        .fields
                        .iter()
                        .map(|field| Record::Struct {
                            name: "Field".to_owned(),
                            fields: vec![
                                ("name".to_owned(), text(&field.name)),
                                ("kind".to_owned(), text(kind_name(field.kind))),
                                (
                                    "ty".to_owned(),
                                    field
                                        .ty
                                        .as_ref()
                                        .map_or(Record::None, |ty| text(&ty.signature())),
                                ),
                                ("default".to_owned(), field.default.clone()),
                            ],
                        })
                        .collect(),
                ),
            ),
        ],
    }
}

/// The declaration [`declaration_to_record`] wrote.
pub(crate) fn declaration_from_record(record: &Record) -> Result<DeclaredRecord, String> {
    fn part<'r>(record: &'r Record, name: &str) -> Result<&'r Record, String> {
        let Record::Struct { fields, .. } = record else {
            return Err("a declaration that is not a struct".to_owned());
        };
        fields
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value)
            .ok_or_else(|| format!("a declaration with no `{name}`"))
    }
    fn text<'r>(record: &'r Record, name: &str) -> Result<&'r str, String> {
        match part(record, name)? {
            Record::Str(value) => Ok(value),
            _ => Err(format!("a declaration's `{name}` that is not text")),
        }
    }
    let domain = text(record, "domain")?;
    let Record::Seq(fields) = part(record, "fields")? else {
        return Err("a declaration's fields that are not a list".to_owned());
    };
    Ok(DeclaredRecord {
        name: text(record, "name")?.to_owned(),
        domain: SemanticDomain::from_name(domain)
            .ok_or_else(|| format!("`{domain}` is not a domain"))?,
        fields: fields
            .iter()
            .map(|field| {
                let kind = text(field, "kind")?;
                let ty = match part(field, "ty")? {
                    Record::None => None,
                    Record::Str(ty) => {
                        Some(ErgonType::parse(ty).ok_or_else(|| format!("`{ty}` is not a type"))?)
                    }
                    _ => return Err("a field type that is not text".to_owned()),
                };
                Ok(DeclaredFieldRecord {
                    name: text(field, "name")?.to_owned(),
                    kind: kind_named(kind)
                        .ok_or_else(|| format!("`{kind}` is not a field kind"))?,
                    ty,
                    default: part(field, "default")?.clone(),
                })
            })
            .collect::<Result<_, String>>()?,
    })
}

/// Every field kind, as a file names it.
const KINDS: [(FieldKind, &str); 10] = [
    (FieldKind::Bool, "Bool"),
    (FieldKind::Int, "Int"),
    (FieldKind::Float, "Float"),
    (FieldKind::Vec2, "Vec2"),
    (FieldKind::Vec3, "Vec3"),
    (FieldKind::Vec4, "Vec4"),
    (FieldKind::Quat, "Quat"),
    (FieldKind::Color, "Color"),
    (FieldKind::Entity, "Entity"),
    (FieldKind::Value, "Value"),
];

pub(crate) fn kind_name(kind: FieldKind) -> &'static str {
    KINDS
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or("Value", |(_, name)| name)
}

pub(crate) fn kind_named(name: &str) -> Option<FieldKind> {
    KINDS
        .iter()
        .find(|(_, known)| *known == name)
        .map(|(kind, _)| *kind)
}

/// The declaration of every saved component `world` declares while it runs,
/// sorted by name: what a record is read under to compare it with `world`.
pub(crate) fn declarations_of(world: &World) -> Vec<DeclaredRecord> {
    let mut declared: Vec<DeclaredRecord> = world
        .components()
        .iter()
        .filter(|vtable| is_saved_declared(vtable))
        .filter_map(declaration_of)
        .collect();
    declared.sort_by(|a, b| a.name.cmp(&b.name));
    declared
}

/// Registers in `world` every one of `declared` it does not have: a scratch
/// world a record is read into under those declarations rather than its own.
pub(crate) fn register_all(world: &mut World, declared: &[DeclaredRecord]) {
    for each in declared {
        let Ok(load) = plan(world, each) else {
            continue;
        };
        if let Some(register) = load.register {
            let _ = world.register_runtime_component(register);
        }
    }
}

/// Whether a scene keeps `vtable`'s component: one declared while the
/// engine runs, that someone authored.
pub(super) fn is_saved_declared(vtable: &ComponentVTable) -> bool {
    vtable.rust.is_none() && vtable.provenance.is_copied_on_duplicate()
}

/// Row `row` of a declared component's `column`, as a record: its fields by
/// name.
pub(super) fn write_value(
    vtable: &ComponentVTable,
    column: &dyn AnyVec,
    row: usize,
    references: &mut dyn ReferenceWriter,
) -> Result<Record, RecordError> {
    let layout = vtable
        .columns
        .packed()
        .ok_or_else(|| RecordError("not a declared component".to_owned()))?;
    let fields = layout
        .fields()
        .iter()
        .enumerate()
        .map(|(slot, field)| {
            let value = vtable
                .columns
                .read_field(column, row, slot)
                .unwrap_or_else(|| field.default.clone());
            Ok((field.name.clone(), encode(&value, references)?))
        })
        .collect::<Result<_, RecordError>>()?;
    Ok(Record::Struct {
        name: vtable.name.to_string(),
        fields,
    })
}

/// `value` as a cell writes it: the tagged script value, its entities
/// through `references`.
fn encode(
    value: &ScriptValue,
    references: &mut dyn ReferenceWriter,
) -> Result<Record, RecordError> {
    to_record(value, references)
}

/// The script value a cell holds.
fn decode(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<ScriptValue, RecordError> {
    from_record::<ScriptValue>(record, references)
}

/// How a load reads one declared component: onto the declaration the world
/// has, or onto the file's, registered when the load commits.
#[derive(Debug)]
pub(crate) struct DeclaredLoad {
    /// The component's name.
    pub(crate) name: Arc<str>,
    /// Its key, the same in every world.
    pub(crate) key: ComponentKey,
    /// The domain the file stored it in.
    pub(crate) file_domain: SemanticDomain,
    /// The domain it lives in once loaded: the world's, else the file's.
    pub(crate) domain: SemanticDomain,
    /// The fields rows are built with, as the load plans them.
    layout: Arc<PackedLayout>,
    /// The file's fields, in its declaration's order: what a snapshot's
    /// positional row holds.
    file_fields: Vec<String>,
    /// The declaration to register when the world has none.
    pub(crate) register: Option<RuntimeComponentDecl>,
}

/// Plans reading the file's `declared` component into `world`. Refused when
/// its name is a Rust component's.
pub(crate) fn plan(world: &World, declared: &DeclaredRecord) -> Result<DeclaredLoad, String> {
    let name = declared.name.as_str();
    let fields = declared
        .fields
        .iter()
        .map(|field| {
            let default = decode(&field.default, &mut Nowhere)
                .map_err(|error| format!("the default of `{name}.{}`: {error}", field.name))?;
            Ok(PackedField {
                name: field.name.clone(),
                kind: field.kind,
                default,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let types = declared
        .fields
        .iter()
        .map(|field| field.ty.clone())
        .collect();
    plan_fields(world, name, declared.domain, fields, types)
}

/// Plans reading a declared component a file describes as `fields` of
/// `types` (each where known), stored in `file_domain`, into `world`.
/// Refused when its name is a Rust component's.
pub(crate) fn plan_fields(
    world: &World,
    name: &str,
    file_domain: SemanticDomain,
    fields: Vec<PackedField>,
    types: Vec<Option<ErgonType>>,
) -> Result<DeclaredLoad, String> {
    let refused = || {
        format!("the file declares `{name}`, a name an engine component has: a file cannot redeclare it")
    };
    if registration_named(name).is_some() {
        return Err(refused());
    }
    let live = match world.components().key_named(name) {
        Some(ComponentKey::Rust(_)) => return Err(refused()),
        Some(key) => world
            .components()
            .vtable(key)
            .and_then(|vtable| Some((vtable.domain, Arc::new(vtable.columns.packed()?.clone())))),
        None => None,
    };
    let file_fields = fields.iter().map(|field| field.name.clone()).collect();
    let (domain, layout, register) = match live {
        Some((domain, layout)) => (domain, layout, None),
        None => {
            let layout = typed_layout(fields, types)
                .map_err(|error| format!("the file's declaration of `{name}`: {error:?}"))?;
            let register = RuntimeComponentDecl {
                name: name.to_owned(),
                domain: file_domain,
                provenance: ComponentProvenance::Authored,
                layout: layout.clone(),
            };
            (file_domain, Arc::new(layout), Some(register))
        }
    };
    Ok(DeclaredLoad {
        name: Arc::from(name),
        key: ComponentKey::named(name),
        file_domain,
        domain,
        layout,
        file_fields,
        register,
    })
}

/// A layout of `fields`, each of its type in `types` where one is known.
fn typed_layout(
    fields: Vec<PackedField>,
    types: Vec<Option<ErgonType>>,
) -> Result<PackedLayout, crate::ecs::LayoutError> {
    if types.iter().any(Option::is_none) {
        return PackedLayout::new(fields);
    }
    PackedLayout::typed(fields, types.into_iter().flatten().collect())
}

impl DeclaredLoad {
    /// Reads one row of the component from `record`: a struct, or the map a
    /// format that writes no struct names reads it back as.
    pub(crate) fn stage_record(
        &self,
        record: &Record,
        references: &mut dyn ReferenceReader,
    ) -> Result<Staged, RecordError> {
        let mut values = Vec::new();
        let mut read = |field: &str, value: &Record| -> Result<(), RecordError> {
            let value = decode(value, references)
                .map_err(|error| RecordError(format!("`{field}`: {error}")))?;
            values.push((field.to_owned(), value));
            Ok(())
        };
        match record {
            Record::Struct { fields, .. } => {
                for (field, value) in fields {
                    read(field, value)?;
                }
            }
            Record::Map(entries) => {
                for (field, value) in entries {
                    let Record::Str(field) = field else {
                        return Err(RecordError(
                            "a field named by something not text".to_owned(),
                        ));
                    };
                    read(field, value)?;
                }
            }
            Record::Unit | Record::UnitStruct { .. } => {}
            _ => return Err(RecordError("not a component's fields".to_owned())),
        }
        Ok(self.staged(values))
    }

    /// One row of the component from its fields' values, in the file's
    /// declaration order — what a snapshot holds.
    pub(crate) fn stage_values(&self, values: Vec<ScriptValue>) -> Staged {
        let named = self.file_fields.iter().cloned().zip(values).collect();
        self.staged(named)
    }

    /// A row of `values` by field name, onto the planned fields, and what
    /// reading it adapted: a field the row lacks takes its default, a value
    /// no field takes is dropped, an `int` for a `float` field is converted
    /// where it converts exactly.
    fn staged(&self, values: Vec<(String, ScriptValue)>) -> Staged {
        let mut report: Vec<ReportEntry> = Vec::new();
        let entry = |report: &mut Vec<ReportEntry>, path: &str, kind: ReportKind| {
            report.push(ReportEntry {
                entity: None,
                component: Some(self.name.to_string()),
                path: path.to_owned(),
                kind,
            });
        };
        let mut kept = Vec::with_capacity(values.len());
        for (field, value) in values {
            let Some(slot) = self.layout.slot_of(&field) else {
                entry(&mut report, &field, ReportKind::Dropped);
                continue;
            };
            match fit(&self.layout, slot, value) {
                Some((value, widened)) => {
                    if widened {
                        entry(&mut report, &field, ReportKind::Widened);
                    }
                    kept.push((field, value));
                }
                None => entry(&mut report, &field, ReportKind::Dropped),
            }
        }
        for field in self.layout.fields() {
            if !kept.iter().any(|(name, _)| *name == field.name)
                && !report.iter().any(|done| done.path == field.name)
            {
                entry(&mut report, &field.name, ReportKind::Defaulted);
            }
        }
        Staged {
            component: Box::new(StagedDeclared {
                key: self.key,
                values: kept,
            }),
            report,
        }
    }
}

/// `value` as field `slot` of `layout` holds it, and whether it was widened:
/// an `int` taken as a `float` only where it converts exactly, and a value
/// of another type than the field's — where its type is known — refused.
fn fit(layout: &PackedLayout, slot: usize, value: ScriptValue) -> Option<(ScriptValue, bool)> {
    let kind = layout.fields()[slot].kind;
    match (kind, &value) {
        (FieldKind::Float, ScriptValue::Int(_)) => {
            Some((FieldKind::Int.widened(kind, &value)?, true))
        }
        _ => Some((layout.admit(slot, &value)?, false)),
    }
}

/// A declared component's row, read: its values by field name, the rest its
/// defaults — placed by the fields of the column it lands in, which are the
/// world's when the load commits.
struct StagedDeclared {
    key: ComponentKey,
    values: Vec<(String, ScriptValue)>,
}

impl StagedComponent for StagedDeclared {
    fn push_into(self: Box<Self>, column: &mut dyn AnyVec) {
        let Some(column) = column.as_any_mut().downcast_mut::<PackedColumn>() else {
            log::error!("a declared component's row met a column that is not packed");
            return;
        };
        column.push_defaults();
        let row = column.len() - 1;
        for (field, value) in self.values {
            let Some(slot) = column.layout().slot_of(&field) else {
                continue;
            };
            // A field the world gave another kind since the load was read
            // keeps its default.
            if let Some((value, _)) = fit(column.layout(), slot, value) {
                let _ = column.set(row, slot, &value);
            }
        }
    }

    fn commit(self: Box<Self>, world: &mut World, entity: EntityId) -> Result<(), String> {
        let key = self.key;
        world
            .attach(entity, key, |column| self.push_into(column))
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }
}

/// Writes every entity as one outside the file: what a declaration's default
/// names is no entity a load could bring.
struct Outside;

impl ReferenceWriter for Outside {
    fn write_entity(&mut self, _: EntityId) -> EntityRef {
        EntityRef::Outside
    }
}

/// An entity no world holds: what a default naming an entity reads as.
const NO_ENTITY: EntityId = EntityId {
    index: u32::MAX,
    generation: u32::MAX,
};

/// Reads every entity as [`NO_ENTITY`]: what a default naming an entity is
/// read as wherever a file holds one.
pub(crate) fn nowhere() -> impl ReferenceReader {
    Nowhere
}

/// Reads every entity as [`NO_ENTITY`] — read before any world is touched,
/// so a refused load reserves nothing.
struct Nowhere;

impl ReferenceReader for Nowhere {
    fn read_entity(&mut self, _: EntityRef) -> Result<EntityId, RecordError> {
        Ok(NO_ENTITY)
    }
}
