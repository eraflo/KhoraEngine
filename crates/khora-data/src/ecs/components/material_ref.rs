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

use bincode::config;
use khora_core::asset::{AssetUUID, Material, StandardMaterial};

use crate::ecs::components::{
    deserialize_material_component, material_from_json, material_to_json,
    serialize_material_component,
};
use crate::ecs::HandleComponent;

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
        let uuid = match serialize_material_component(material.base_color(), &*material) {
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

/// On-disk form of a [`MaterialRef`]. The bincode discriminant distinguishes
/// the two arms; `Inline` holds the opaque type-tagged material bytes produced
/// by [`serialize_material_component`], `Asset` holds the raw UUID (preserved
/// verbatim, never regenerated).
#[derive(bincode::Encode, bincode::Decode)]
enum SerializableMaterialRef {
    /// Type-tagged material payload (`SerializableMaterialData` bytes).
    Inline(Vec<u8>),
    /// VFS asset UUID, round-tripped unchanged.
    Asset(AssetUUID),
}

/// Serializes the entity's `MaterialRef` into the recipe byte stream.
fn serialize_material_ref(
    world: &crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
) -> Option<Vec<u8>> {
    let mref = world.get::<MaterialRef>(entity)?;
    let on_disk = match mref {
        MaterialRef::Inline { material, .. } => {
            let bytes = serialize_material_component(material.base_color(), &**material)?;
            SerializableMaterialRef::Inline(bytes)
        }
        MaterialRef::Asset(uuid) => SerializableMaterialRef::Asset(*uuid),
    };
    bincode::encode_to_vec(&on_disk, config::standard()).ok()
}

/// Reconstructs a `MaterialRef` from recipe bytes and attaches it to `entity`.
fn deserialize_material_ref(
    world: &mut crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
    data: &[u8],
) -> Result<(), String> {
    let (on_disk, _): (SerializableMaterialRef, _) =
        bincode::decode_from_slice(data, config::standard()).map_err(|e| e.to_string())?;
    let mref = match on_disk {
        SerializableMaterialRef::Inline(bytes) => {
            let (handle, _uuid) = deserialize_material_component(&bytes)?;
            MaterialRef::inline(handle.clone_box())
        }
        SerializableMaterialRef::Asset(uuid) => MaterialRef::Asset(uuid),
    };
    world
        .add_component(entity, mref)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
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
        provenance: crate::ecs::ComponentProvenance::Authored,
        serialize_recipe: serialize_material_ref,
        deserialize_recipe: deserialize_material_ref,
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
