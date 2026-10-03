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

//! Authored, serialized reference to a material.
//!
//! `MaterialRef` is the *only* serialized material form. It is an explicit
//! discriminator the developer/editor sets:
//!
//! - [`MaterialRef::Inline`] embeds an ad-hoc material value (editor- or
//!   code-created) directly in the scene.
//! - [`MaterialRef::Asset`] references a `.kmat` file in the VFS by its stable
//!   `AssetUUID`.
//!
//! A registered resolver `DataSystem` (in `khora-io`) turns a `MaterialRef`
//! into the runtime-only [`MaterialHandle`] (a `HandleComponent<Box<dyn
//! Material>>`), which the GPU projection consumes. `MaterialRef` itself never
//! touches the GPU and never carries a resolved handle.

use khora_core::asset::{AssetUUID, Material, StandardMaterial};

use crate::ecs::HandleComponent;
use crate::scene::record::{Record, RecordError, ReferenceReader, ReferenceWriter, VariantPayload};
use crate::scene::{
    material_from_json, material_from_record, material_to_json, material_to_record,
};

/// Runtime-only resolved material: a shared handle to the type-erased material
/// data plus its identifying `AssetUUID`. Produced by the resolver, consumed by
/// the GPU material projection. A readable alias for call sites.
pub type MaterialHandle = HandleComponent<Box<dyn Material>>;

/// Authored reference to a material — the only serialized material form.
///
/// The enum *is* the discriminator: `Inline` embeds the material value,
/// `Asset` carries the stable UUID of a `.kmat` in the VFS.
///
/// Both arms carry an *identity UUID* — the content-derived UUID for `Inline`,
/// the stable asset UUID for `Asset`. The resolver compares this identity
/// against the resolved handle's UUID every tick: a mismatch (or a missing
/// handle) means the authored reference changed and must be re-resolved. The
/// `Inline` UUID is never serialized; it is recomputed from the material
/// content on deserialize via [`MaterialRef::inline`] so it stays purely
/// content-derived. Construct `Inline` through [`MaterialRef::inline`] so the
/// UUID and the material value can never disagree.
pub enum MaterialRef {
    /// Ad-hoc material data created in code or the editor, embedded inline.
    /// The `uuid` is content-derived; build via [`MaterialRef::inline`].
    Inline {
        /// The embedded material value.
        material: Box<dyn Material>,
        /// Content-derived identity UUID (the same content hash the GPU
        /// projection keys on). Not serialized — recomputed on load.
        uuid: AssetUUID,
    },
    /// Reference to a `.kmat` asset in the VFS, resolved by UUID.
    Asset(AssetUUID),
}

impl MaterialRef {
    /// Builds an `Inline` reference from a material value, deriving its identity
    /// UUID from the material content. Two inline references holding equal
    /// material content get the same UUID, so they dedup to one resolved handle
    /// (and one `GpuMaterial`). On a serialization failure the UUID falls back
    /// to a fresh random value (logged) rather than panicking.
    pub fn inline(material: Box<dyn Material>) -> Self {
        // Hashed from the material's JSON form: deterministic for equal
        // content, and the same form the inspector and `.kmat` files use.
        let content = material_to_json(&*material).and_then(|json| serde_json::to_vec(&json).ok());
        let uuid = match content {
            Some(bytes) => AssetUUID::new_v5(&blake3::hash(&bytes).to_hex()),
            None => {
                log::error!(
                    "MaterialRef::inline: failed to serialize material for identity UUID; \
                     using a random UUID (this inline material will not dedup)."
                );
                AssetUUID::new()
            }
        };
        Self::Inline { material, uuid }
    }

    /// Returns the identity UUID: the content-derived UUID for `Inline`, the
    /// stable asset UUID for `Asset`. The resolver compares this against the
    /// resolved handle's UUID to detect an authored change.
    pub fn uuid(&self) -> AssetUUID {
        match self {
            Self::Inline { uuid, .. } => *uuid,
            Self::Asset(uuid) => *uuid,
        }
    }
}

impl Clone for MaterialRef {
    fn clone(&self) -> Self {
        match self {
            Self::Inline { material, uuid } => Self::Inline {
                material: material.clone_box(),
                uuid: *uuid,
            },
            Self::Asset(uuid) => Self::Asset(*uuid),
        }
    }
}

impl crate::ecs::Component for MaterialRef {}

/// Writes the entity's `MaterialRef` as a record.
///
/// `Inline` names its material's type beside the material, rather than
/// relying on the struct's own name: a self-describing format such as JSON
/// keeps field names but not struct names, and the type is what picks the
/// registration that reads it back. `Asset` holds the stable UUID.
fn material_ref_to_record(
    column: &dyn crate::ecs::AnyVec,
    row: usize,
    references: &mut dyn ReferenceWriter,
) -> Result<Record, RecordError> {
    let mref = <MaterialRef as crate::ecs::Component>::clone_from_column(column, row);
    let (variant, payload) = match &mref {
        MaterialRef::Inline { material, .. } => {
            let (type_name, record) = material_to_record(&**material, references)?;
            (
                "Inline",
                VariantPayload::Struct(vec![
                    ("type_name".to_owned(), Record::Str(type_name.to_owned())),
                    ("material".to_owned(), record),
                ]),
            )
        }
        MaterialRef::Asset(uuid) => (
            "Asset",
            VariantPayload::Newtype(Box::new(Record::Asset(*uuid))),
        ),
    };
    Ok(Record::Variant {
        enum_name: "MaterialRef".to_owned(),
        variant: variant.to_owned(),
        payload,
    })
}

/// Reads a `MaterialRef` back from a record — as written, or as a
/// self-describing format presents a variant (a map keyed by its name).
fn stage_material_ref(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<crate::scene::Staged, RecordError> {
    let (variant, carried): (&str, Record) = match record {
        Record::Variant {
            variant, payload, ..
        } => (
            variant,
            match payload {
                VariantPayload::Newtype(inner) => (**inner).clone(),
                VariantPayload::Struct(fields) => Record::Struct {
                    name: String::new(),
                    fields: fields.clone(),
                },
                VariantPayload::Tuple(items) => Record::Seq(items.clone()),
                VariantPayload::Unit => Record::Unit,
            },
        ),
        Record::Map(entries) if entries.len() == 1 => match &entries[0] {
            (Record::Str(variant), value) => (variant.as_str(), value.clone()),
            _ => {
                return Err(RecordError(
                    "a material reference keyed by a non-name".to_owned(),
                ))
            }
        },
        _ => {
            return Err(RecordError(
                "a material reference must be `Inline` or `Asset`".to_owned(),
            ))
        }
    };
    let field = |name: &str| -> Option<&Record> {
        match &carried {
            Record::Struct { fields, .. } => {
                fields.iter().find(|(key, _)| key == name).map(|(_, v)| v)
            }
            Record::Map(entries) => entries.iter().find_map(|(key, value)| match key {
                Record::Str(key) if key == name => Some(value),
                _ => None,
            }),
            _ => None,
        }
    };
    let (mref, report) = match variant {
        "Asset" => match &carried {
            Record::Asset(uuid) => (MaterialRef::Asset(*uuid), Vec::new()),
            _ => {
                return Err(RecordError(
                    "a material asset reference without its UUID".to_owned(),
                ))
            }
        },
        "Inline" => {
            let type_name = match field("type_name") {
                Some(Record::Str(type_name)) => type_name.clone(),
                _ => {
                    return Err(RecordError(
                        "an inline material without its type name".to_owned(),
                    ))
                }
            };
            let material = field("material")
                .ok_or_else(|| RecordError("an inline material without its value".to_owned()))?;
            let (material, mut report) = material_from_record(&type_name, material, references)?;
            for entry in &mut report {
                entry.path = if entry.path.is_empty() {
                    "Inline.material".to_owned()
                } else {
                    format!("Inline.material.{}", entry.path)
                };
            }
            (MaterialRef::inline(material), report)
        }
        other => {
            return Err(RecordError(format!(
                "no material reference variant `{other}`"
            )))
        }
    };
    Ok(crate::scene::Staged {
        component: Box::new(crate::scene::StagedValue(mref)),
        report,
    })
}

/// Editor JSON form: `Inline` reuses `material_to_json`; `Asset` emits
/// `{ "asset": "<uuid>" }`.
fn material_ref_to_json(
    world: &crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
) -> Option<serde_json::Value> {
    let mref = world.get::<MaterialRef>(entity)?;
    match mref {
        MaterialRef::Inline { material, .. } => material_to_json(&**material),
        MaterialRef::Asset(uuid) => serde_json::to_value(uuid)
            .ok()
            .map(|uuid_json| serde_json::json!({ "asset": uuid_json })),
    }
}

/// Parses the editor JSON form back into a `MaterialRef` and applies it.
fn material_ref_from_json(
    world: &mut crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
    value: &serde_json::Value,
) -> Result<(), String> {
    let mref = if let Some(asset) = value.get("asset") {
        let uuid: AssetUUID = serde_json::from_value(asset.clone()).map_err(|e| e.to_string())?;
        MaterialRef::Asset(uuid)
    } else {
        let (handle, _uuid) = material_from_json(value)?;
        MaterialRef::inline(handle.clone_box())
    };
    if !world.set_component(entity, mref.clone()) {
        world
            .add_component(entity, mref)
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

inventory::submit! {
    crate::scene::ComponentRegistration {
        type_id: std::any::TypeId::of::<MaterialRef>(),
        type_name: "MaterialRef",
        // An enum: one-of, not all-of. See `ComponentShape`.
        shape: crate::scene::ComponentShape::Opaque,
        provenance: crate::ecs::ComponentProvenance::Authored,
        formerly: &[],
        column_to_record: material_ref_to_record,
        stage: stage_material_ref,
        create_default: |world, entity| {
            world
                .add_component(
                    entity,
                    MaterialRef::inline(Box::new(StandardMaterial::default())),
                )
                .map_err(|e| format!("{e:?}"))?;
            Ok(())
        },
        to_json: material_ref_to_json,
        from_json: material_ref_from_json,
        remove: |world, entity| {
            match world.remove_component::<MaterialRef>(entity) {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{e:?}")),
            }
        },
    }
}
