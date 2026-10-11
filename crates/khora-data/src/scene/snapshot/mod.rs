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

//! A world as a snapshot: the fastest file to load, bound to the schema that
//! wrote it.
//!
//! Values are positional — no names, no record tree, no name resolution on
//! load — and every component is listed with its schema fingerprint. A
//! snapshot whose fingerprints differ from the running registrations is
//! refused whole: it is the same build's cache of a stable save, never its
//! only copy.
//!
//! Payload: `version: u8`; the component table — a count, then each name
//! (length and UTF-8) and its fingerprint (`u64` LE); the entities — a
//! count, then each persistent id (`u64` LE); the pages — a count, then for
//! each its components (a count, then table indices), its rows (a count,
//! then persistent ids) and its values column after column, each value its
//! length then its bytes. Counts and lengths are LEB128 varints.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::PersistentId;
use khora_core::scene::SceneFile;
use khora_core::script::{ErgonType, ScriptValue};

use super::apply::{failure, prepare_pages, to_register, LoadFailure, Loaded, PagePlan, Prepared};
use super::capture::{capture_pages, world_in_tree_order, SaveError, Saved};
use super::component_registration::{ComponentRegistration, Kept};
use super::declared::{kind_name, kind_named, nowhere, plan_fields, DeclaredLoad};
use super::file::{check_format, encoding_of, scene_file};
use super::positional::{from_positional, to_positional, write_varint};
use super::record::{LoadReport, RecordError};
use crate::ecs::{ComponentKey, PackedField, SemanticDomain, World};

/// The id a scene header names the snapshot encoding by.
pub const SNAPSHOT_ENCODING_ID: &str = "KH_SNAPSHOT_V1";

/// The payload layout this module writes and reads.
///
/// Version 2 lists, beside each Rust component and its fingerprint, each
/// component declared while the engine runs with its fields — read back onto
/// the world's declaration by name, since a hot reload may have given it new
/// fields since the snapshot was taken.
const VERSION: u8 = 2;

/// A table entry naming a Rust component, guarded by its fingerprint —
/// the only entry layout 1, which still reads, has.
const RUST_ENTRY: u8 = 0;
/// A table entry describing a component declared while the engine runs.
const DECLARED_ENTRY: u8 = 1;

/// Writes every entity of `world` and its saved components as a snapshot.
pub fn write_snapshot(world: &World) -> Result<SceneFile, SaveError> {
    let entities = world_in_tree_order(world);
    let (ids, pages) = capture_pages(
        world,
        &entities,
        None,
        Kept::Scene,
        |saved, column, row, references| {
            let mut bytes = Vec::new();
            match saved {
                Saved::Rust(reg) => (reg.column_to_snapshot)(column, row, &mut bytes, references)?,
                // Every field, in the declaration's order.
                Saved::Declared(vtable) => {
                    let fields = vtable
                        .columns
                        .packed()
                        .map_or(0, |layout| layout.fields().len());
                    let values: Vec<ScriptValue> = (0..fields)
                        .map(|slot| {
                            vtable
                                .columns
                                .read_field(column, row, slot)
                                .unwrap_or(ScriptValue::Unit)
                        })
                        .collect();
                    to_positional(&values, &mut bytes, references)?;
                }
            }
            Ok(bytes)
        },
    )?;

    // Every component the pages hold, once, in the order first met — each
    // Rust one a fingerprint can guard.
    let mut table: Vec<Saved> = Vec::new();
    let mut index_of: HashMap<ComponentKey, usize> = HashMap::new();
    for page in &pages {
        for saved in &page.components {
            if let std::collections::hash_map::Entry::Vacant(slot) = index_of.entry(saved.key()) {
                if let Saved::Rust(reg) = saved {
                    if !(reg.schema_complete)() {
                        return Err(SaveError::Unguarded(reg.type_name.to_owned()));
                    }
                }
                slot.insert(table.len());
                table.push(*saved);
            }
        }
    }

    let mut payload = vec![VERSION];
    write_varint(&mut payload, table.len() as u64);
    for saved in &table {
        let name = saved.name();
        write_varint(&mut payload, name.len() as u64);
        payload.extend_from_slice(name.as_bytes());
        match saved {
            Saved::Rust(reg) => {
                payload.push(RUST_ENTRY);
                payload.extend_from_slice(&(reg.schema)().to_le_bytes());
            }
            Saved::Declared(vtable) => {
                payload.push(DECLARED_ENTRY);
                payload.push(vtable.domain.index() as u8);
                let fields = vtable
                    .columns
                    .packed()
                    .map_or(&[][..], |layout| layout.fields());
                write_varint(&mut payload, fields.len() as u64);
                for (slot, field) in fields.iter().enumerate() {
                    let ty = vtable
                        .columns
                        .packed()
                        .and_then(|layout| layout.ty(slot))
                        .map(|ty| ty.signature())
                        .unwrap_or_default();
                    for text in [field.name.as_str(), kind_name(field.kind), ty.as_str()] {
                        write_varint(&mut payload, text.len() as u64);
                        payload.extend_from_slice(text.as_bytes());
                    }
                    let mut default = Vec::new();
                    to_positional(&field.default, &mut default, &mut Outside).map_err(|error| {
                        SaveError::Component {
                            component: name.to_owned(),
                            error,
                        }
                    })?;
                    write_varint(&mut payload, default.len() as u64);
                    payload.extend_from_slice(&default);
                }
            }
        }
    }
    write_varint(&mut payload, ids.len() as u64);
    for id in &ids {
        payload.extend_from_slice(&id.to_bits().to_le_bytes());
    }
    write_varint(&mut payload, pages.len() as u64);
    for page in &pages {
        write_varint(&mut payload, page.components.len() as u64);
        for saved in &page.components {
            write_varint(&mut payload, index_of[&saved.key()] as u64);
        }
        write_varint(&mut payload, page.rows.len() as u64);
        for id in &page.rows {
            payload.extend_from_slice(&id.to_bits().to_le_bytes());
        }
        // Column by column, as the page stores them.
        for column in &page.columns {
            for value in column {
                write_varint(&mut payload, value.len() as u64);
                payload.extend_from_slice(value);
            }
        }
    }
    scene_file(SNAPSHOT_ENCODING_ID, payload).map_err(|error| SaveError::Encoding(error.0))
}

/// Reads and checks the snapshot `file` holds against `world`'s
/// registrations, reserving an id for each of its entities, without adding
/// anything — committed through the shared [`Prepared::commit`].
pub fn prepare_snapshot(world: &mut World, file: &SceneFile) -> Result<Prepared, LoadFailure> {
    let refused = |message: String| failure(message, LoadReport::default());
    let encoding = encoding_of(file);
    if encoding != SNAPSHOT_ENCODING_ID {
        return Err(refused(format!("a `{encoding}` file is not a snapshot")));
    }
    check_format(file).map_err(|error| refused(error.to_string()))?;
    let snapshot = Snapshot::read(&file.payload).map_err(refused)?;

    // Each declared component read onto the world's declaration, or
    // registered from the snapshot's when the world has none.
    let mut declared = HashMap::new();
    let mut table = Vec::with_capacity(snapshot.table.len());
    for entry in &snapshot.table {
        table.push(match entry {
            Entry::Rust(reg) => Loaded::Rust(reg),
            Entry::Declared {
                name,
                domain,
                fields,
                types,
            } => {
                let load = Arc::new(
                    plan_fields(world, name, *domain, fields.clone(), types.clone(), false)
                        .map_err(refused)?,
                );
                if declared.insert(name.clone(), Arc::clone(&load)).is_some() {
                    return Err(refused(format!("the snapshot lists `{name}` twice")));
                }
                Loaded::Declared(load)
            }
        });
    }

    let plans = snapshot
        .pages
        .iter()
        .map(|page| PagePlan {
            rows: &page.rows,
            columns: page
                .components
                .iter()
                .enumerate()
                .map(|(column, &entry)| (column, table[entry].clone()))
                .collect(),
        })
        .collect();
    let mut prepared = prepare_pages(
        world,
        &snapshot.entities,
        plans,
        LoadReport::default(),
        |loaded, page, column, row, references| {
            let bytes = snapshot.pages[page].values[column][row];
            match loaded {
                Loaded::Rust(reg) => (reg.stage_snapshot)(bytes, references),
                Loaded::Declared(load) => {
                    let values: Vec<ScriptValue> = from_positional(bytes, references)?;
                    Ok(stage_declared(load, values))
                }
            }
        },
    )?;
    let used: Vec<String> = declared.keys().cloned().collect();
    prepared.set_register(to_register(&declared, used.iter().map(String::as_str)));
    Ok(prepared)
}

/// A declared component's row, from the values the snapshot holds.
fn stage_declared(load: &DeclaredLoad, values: Vec<ScriptValue>) -> super::Staged {
    load.stage_values(values)
}

/// A component the snapshot's table lists.
enum Entry {
    /// A Rust component, its fingerprint the one this build has.
    Rust(&'static ComponentRegistration),
    /// A component declared while the engine ran.
    Declared {
        name: String,
        domain: SemanticDomain,
        fields: Vec<PackedField>,
        types: Vec<Option<ErgonType>>,
    },
}

/// Writes every entity as one outside the snapshot: what a declaration's
/// default names is no entity a load could bring.
struct Outside;

impl super::record::ReferenceWriter for Outside {
    fn write_entity(&mut self, _: khora_core::ecs::entity::EntityId) -> super::record::EntityRef {
        super::record::EntityRef::Outside
    }
}

/// A snapshot's payload, read and checked, its values not yet.
struct Snapshot<'p> {
    table: Vec<Entry>,
    entities: Vec<PersistentId>,
    pages: Vec<SnapshotPage<'p>>,
}

struct SnapshotPage<'p> {
    /// Indices into the component table.
    components: Vec<usize>,
    rows: Vec<PersistentId>,
    /// Each column's values, a byte slice per row.
    values: Vec<Vec<&'p [u8]>>,
}

impl<'p> Snapshot<'p> {
    fn read(payload: &'p [u8]) -> Result<Self, String> {
        let mut input = Input {
            bytes: payload,
            at: 0,
        };
        let version = input.byte()?;
        if !(1..=VERSION).contains(&version) {
            return Err(format!(
                "snapshot layout {version}, this engine reads up to {VERSION}"
            ));
        }

        // A name's length, a name, an entry kind and what it holds.
        let count = input.count(3)?;
        let mut table = Vec::with_capacity(count);
        for _ in 0..count {
            let name = input.text()?;
            // Layout 1 lists Rust components only, with no entry kind.
            let kind = if version == 1 {
                RUST_ENTRY
            } else {
                input.byte()?
            };
            table.push(match kind {
                RUST_ENTRY => Entry::Rust(registration_for(name, input.u64_le()?)?),
                DECLARED_ENTRY => {
                    let domain = SemanticDomain::ALL
                        .get(usize::from(input.byte()?))
                        .copied()
                        .ok_or_else(|| {
                            format!("`{name}` is stored in no domain this engine has")
                        })?;
                    let count = input.count(3)?;
                    let mut fields = Vec::with_capacity(count);
                    let mut types = Vec::with_capacity(count);
                    for _ in 0..count {
                        let field = input.text()?.to_owned();
                        let kind = input.text()?;
                        let kind = kind_named(kind)
                            .ok_or_else(|| format!("`{name}.{field}` is of no kind: `{kind}`"))?;
                        let ty = match input.text()? {
                            "" => None,
                            ty => Some(ErgonType::parse(ty).ok_or_else(|| {
                                format!("`{name}.{field}` is of no type: `{ty}`")
                            })?),
                        };
                        let len = input.count(1)?;
                        let default: ScriptValue =
                            from_positional(input.take(len)?, &mut nowhere()).map_err(
                                |RecordError(error)| {
                                    format!("the default of `{name}.{field}`: {error}")
                                },
                            )?;
                        fields.push(PackedField {
                            name: field,
                            kind,
                            default,
                        });
                        types.push(ty);
                    }
                    Entry::Declared {
                        name: name.to_owned(),
                        domain,
                        fields,
                        types,
                    }
                }
                other => return Err(format!("`{name}` has a table entry of kind {other}")),
            });
        }

        let count = input.count(8)?;
        let mut entities = Vec::with_capacity(count);
        for _ in 0..count {
            entities.push(PersistentId::from_bits(input.u64_le()?));
        }

        let count = input.count(3)?;
        let mut pages = Vec::with_capacity(count);
        for _ in 0..count {
            let columns = input.count(1)?;
            let mut components = Vec::with_capacity(columns);
            for _ in 0..columns {
                let index = usize::try_from(input.varint()?).unwrap_or(usize::MAX);
                if index >= table.len() {
                    return Err(format!("component {index} is not in the table"));
                }
                components.push(index);
            }
            let count = input.count(8)?;
            let mut rows = Vec::with_capacity(count);
            for _ in 0..count {
                rows.push(PersistentId::from_bits(input.u64_le()?));
            }
            // Every value takes at least its length byte.
            if columns
                .checked_mul(rows.len())
                .is_none_or(|values| values > input.left())
            {
                return Err(format!(
                    "a page of {} rows by {columns} columns the file has no room for",
                    rows.len()
                ));
            }
            let mut values = Vec::with_capacity(columns);
            for _ in 0..columns {
                let mut column = Vec::with_capacity(rows.len());
                for _ in 0..rows.len() {
                    let len = input.count(1)?;
                    column.push(input.take(len)?);
                }
                values.push(column);
            }
            pages.push(SnapshotPage {
                components,
                rows,
                values,
            });
        }
        if input.left() > 0 {
            return Err(format!("{} byte(s) after the last page", input.left()));
        }
        Ok(Self {
            table,
            entities,
            pages,
        })
    }
}

/// The registration a snapshot's component is read with: this build's own
/// name for it, its schema the one that wrote it, a component a save holds.
fn registration_for(
    name: &str,
    fingerprint: u64,
) -> Result<&'static ComponentRegistration, String> {
    let reg = inventory::iter::<ComponentRegistration>
        .into_iter()
        .find(|reg| reg.type_name == name)
        .ok_or_else(|| format!("unknown component type `{name}` in a snapshot"))?;
    if !reg.is_saved() {
        return Err(format!(
            "component `{name}` is derived or runtime state; a snapshot does not hold it"
        ));
    }
    if !(reg.schema_complete)() {
        return Err(format!(
            "component `{name}` has a schema no fingerprint can guard; a snapshot does not hold it"
        ));
    }
    let current = (reg.schema)();
    if current != fingerprint {
        return Err(format!(
            "component `{name}` was written by another schema \
             ({fingerprint:016x}, this build has {current:016x}): load its stable save"
        ));
    }
    Ok(reg)
}

/// A cursor over the payload. Every count is checked against the bytes left
/// before anything is sized from it: a count is the file's claim.
struct Input<'p> {
    bytes: &'p [u8],
    at: usize,
}

impl<'p> Input<'p> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or_else(|| "the snapshot ends early".to_owned())?;
        self.at += 1;
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&'p [u8], String> {
        if len > self.left() {
            return Err("the snapshot ends early".to_owned());
        }
        let taken = &self.bytes[self.at..self.at + len];
        self.at += len;
        Ok(taken)
    }

    /// A length, then that many bytes of UTF-8.
    fn text(&mut self) -> Result<&'p str, String> {
        let len = self.count(1)?;
        std::str::from_utf8(self.take(len)?).map_err(|_| "a name that is not UTF-8".to_owned())
    }

    fn u64_le(&mut self) -> Result<u64, String> {
        let mut array = [0; 8];
        array.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(array))
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return Err("a number too large for 64 bits".to_owned());
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a number too large for 64 bits".to_owned())
    }

    /// A count of things each taking at least `each` bytes.
    fn count(&mut self, each: usize) -> Result<usize, String> {
        let count = self.varint()?;
        let count = usize::try_from(count).map_err(|_| "a count too large".to_owned())?;
        if count
            .checked_mul(each)
            .is_none_or(|needed| needed > self.left())
        {
            return Err(format!("a count of {count} the snapshot has no room for"));
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
