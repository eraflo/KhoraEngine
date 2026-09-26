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

//! Authored, serialized reference to a mesh.
//!
//! `MeshRef` is the *only* serialized mesh form. It is an explicit
//! discriminator the developer/editor sets:
//!
//! - [`MeshRef::Procedural`] carries the kind + generator parameters of a
//!   primitive (cube, sphere, plane), regenerated on resolution — no
//!   structural fingerprinting.
//! - [`MeshRef::Asset`] references an imported mesh file (glTF, OBJ) in the
//!   VFS by its stable `AssetUUID`.
//!
//! A registered resolver `DataSystem` (in `khora-io`) turns a `MeshRef` into
//! the runtime-only `HandleComponent<Mesh>`, which the GPU mesh projection
//! consumes. `MeshRef` itself never touches the GPU and never carries a
//! resolved handle.
//!
//! The procedural geometry generators ([`reconstruct_procedural_mesh`] and the
//! `create_*` builders) live in `assets/procedural_mesh.rs` and are public so the
//! resolver can rebuild a `Mesh` from a [`MeshRef::Procedural`].
//!
//! [`reconstruct_procedural_mesh`]: crate::ecs::reconstruct_procedural_mesh

use bincode::{config, Decode, Encode};
use khora_core::asset::AssetUUID;

/// Identifies a known procedural mesh primitive.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, serde::Serialize, serde::Deserialize,
)]
pub enum ProceduralMeshKind {
    /// Axis-aligned cube primitive.
    Cube,
    /// UV sphere primitive.
    Sphere,
    /// Flat XZ plane primitive.
    Plane,
}

/// Authored reference to a mesh — the only serialized mesh form.
///
/// The enum *is* the discriminator: `Procedural` carries the primitive kind
/// and its generator parameters; `Asset` carries the stable UUID of an
/// imported mesh in the VFS.
///
/// Both arms carry an *identity UUID* — the content-derived UUID for
/// `Procedural`, the stable asset UUID for `Asset`. The resolver compares this
/// against the resolved handle's UUID every tick: a mismatch (or a missing
/// handle) means the authored reference changed and must be re-resolved. The
/// `Procedural` UUID is never serialized; it is recomputed from `kind` +
/// `params` on deserialize via [`MeshRef::procedural`] so it stays purely
/// content-derived. Construct `Procedural` through [`MeshRef::procedural`] so
/// the UUID and the parameters can never disagree.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshRef {
    /// Procedural primitive — rebuilt from `kind` + `params` by the resolver.
    /// The `uuid` is content-derived; build via [`MeshRef::procedural`].
    Procedural {
        /// Which procedural primitive to regenerate.
        kind: ProceduralMeshKind,
        /// Generator parameters (kind-specific layout, padded with zeros).
        params: [f32; 4],
        /// Content-derived identity UUID. Not serialized — recomputed on load.
        uuid: AssetUUID,
    },
    /// Reference to an imported mesh asset in the VFS, resolved by UUID.
    Asset(AssetUUID),
}

impl crate::ecs::Component for MeshRef {}

impl MeshRef {
    /// Builds a `Procedural` reference, deriving its identity UUID from the
    /// primitive `kind` discriminant plus the raw parameter bytes. Identical
    /// procedural meshes get the same UUID, so they dedup to one resolved
    /// handle (and one `GpuMesh`). This is the SAME content hash the GPU
    /// projection keys on.
    pub fn procedural(kind: ProceduralMeshKind, params: [f32; 4]) -> Self {
        let mut key = Vec::with_capacity(1 + 16);
        key.push(match kind {
            ProceduralMeshKind::Cube => 0u8,
            ProceduralMeshKind::Sphere => 1,
            ProceduralMeshKind::Plane => 2,
        });
        for p in params {
            key.extend_from_slice(&p.to_le_bytes());
        }
        let uuid = AssetUUID::new_v5(&blake3::hash(&key).to_hex());
        Self::Procedural { kind, params, uuid }
    }

    /// Returns the identity UUID: the content-derived UUID for `Procedural`,
    /// the stable asset UUID for `Asset`. The resolver compares this against the
    /// resolved handle's UUID to detect an authored change.
    pub fn uuid(&self) -> AssetUUID {
        match self {
            Self::Procedural { uuid, .. } => *uuid,
            Self::Asset(uuid) => *uuid,
        }
    }

    /// A unit cube — the default authored mesh.
    pub fn unit_cube() -> Self {
        Self::procedural(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0])
    }
}

/// On-disk form of a [`MeshRef`]. The `Procedural` arm omits the identity
/// UUID — it is recomputed from `kind` + `params` on load via
/// [`MeshRef::procedural`] so it always stays content-derived. The `Asset`
/// arm round-trips its UUID verbatim.
#[derive(Encode, Decode, serde::Serialize, serde::Deserialize)]
enum SerializableMeshRef {
    /// Procedural primitive parameters (UUID recomputed on load).
    Procedural {
        /// Which procedural primitive to regenerate.
        kind: ProceduralMeshKind,
        /// Generator parameters (kind-specific layout, padded with zeros).
        params: [f32; 4],
    },
    /// VFS asset UUID, round-tripped unchanged.
    Asset(AssetUUID),
}

impl From<&MeshRef> for SerializableMeshRef {
    fn from(mesh_ref: &MeshRef) -> Self {
        match mesh_ref {
            MeshRef::Procedural { kind, params, .. } => Self::Procedural {
                kind: *kind,
                params: *params,
            },
            MeshRef::Asset(uuid) => Self::Asset(*uuid),
        }
    }
}

impl From<SerializableMeshRef> for MeshRef {
    fn from(on_disk: SerializableMeshRef) -> Self {
        match on_disk {
            SerializableMeshRef::Procedural { kind, params } => Self::procedural(kind, params),
            SerializableMeshRef::Asset(uuid) => Self::Asset(uuid),
        }
    }
}

/// Serializes the entity's `MeshRef` into the recipe byte stream.
fn serialize_mesh_ref(
    world: &crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
) -> Option<Vec<u8>> {
    let mesh_ref = world.get::<MeshRef>(entity)?;
    let on_disk = SerializableMeshRef::from(mesh_ref);
    bincode::encode_to_vec(&on_disk, config::standard()).ok()
}

/// Reconstructs a `MeshRef` from recipe bytes and attaches it to `entity`.
fn deserialize_mesh_ref(
    world: &mut crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
    data: &[u8],
) -> Result<(), String> {
    let (on_disk, _): (SerializableMeshRef, _) =
        bincode::decode_from_slice(data, config::standard()).map_err(|e| e.to_string())?;
    world
        .add_component(entity, MeshRef::from(on_disk))
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

/// Editor JSON form — the on-disk `MeshRef` shape (UUID omitted for
/// `Procedural`, recomputed on parse).
fn mesh_ref_to_json(
    world: &crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
) -> Option<serde_json::Value> {
    let mesh_ref = world.get::<MeshRef>(entity)?;
    serde_json::to_value(SerializableMeshRef::from(mesh_ref)).ok()
}

/// Parses the editor JSON form back into a `MeshRef` and applies it.
fn mesh_ref_from_json(
    world: &mut crate::ecs::World,
    entity: khora_core::ecs::entity::EntityId,
    value: &serde_json::Value,
) -> Result<(), String> {
    let on_disk: SerializableMeshRef =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    let mesh_ref = MeshRef::from(on_disk);
    if !world.set_component(entity, mesh_ref.clone()) {
        world
            .add_component(entity, mesh_ref)
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

inventory::submit! {
    crate::scene::ComponentRegistration {
        type_id: std::any::TypeId::of::<MeshRef>(),
        type_name: "MeshRef",
        // An enum: one-of, not all-of. See `ComponentShape`.
        shape: crate::scene::ComponentShape::Opaque,
        provenance: crate::ecs::ComponentProvenance::Authored,
        serialize_recipe: serialize_mesh_ref,
        deserialize_recipe: deserialize_mesh_ref,
        create_default: |world, entity| {
            world
                .add_component(entity, MeshRef::unit_cube())
                .map_err(|e| format!("{e:?}"))?;
            Ok(())
        },
        to_json: mesh_ref_to_json,
        from_json: mesh_ref_from_json,
        remove: |world, entity| {
            match world.remove_component::<MeshRef>(entity) {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{e:?}")),
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::World;
    use crate::scene::registry::ComponentRegistration;

    /// `MeshRef::Procedural` survives a recipe serialize → deserialize cycle.
    #[test]
    fn procedural_mesh_ref_recipe_round_trip() {
        let mut src = World::new();
        let entity = src.spawn(MeshRef::procedural(
            ProceduralMeshKind::Sphere,
            [0.75, 32.0, 16.0, 0.0],
        ));

        let reg = inventory::iter::<ComponentRegistration>
            .into_iter()
            .find(|r| r.type_name == "MeshRef")
            .expect("MeshRef registration present");
        let bytes = (reg.serialize_recipe)(&src, entity).expect("serialize");

        let mut dst = World::new();
        let new_entity = dst.spawn(());
        (reg.deserialize_recipe)(&mut dst, new_entity, &bytes).expect("deserialize");

        let restored = dst.get::<MeshRef>(new_entity).expect("mesh ref restored");
        // The recomputed identity UUID matches the original (content-derived).
        assert_eq!(
            restored,
            &MeshRef::procedural(ProceduralMeshKind::Sphere, [0.75, 32.0, 16.0, 0.0])
        );
    }

    /// `MeshRef::Asset(uuid)` round-trips with the SAME uuid preserved.
    #[test]
    fn asset_mesh_ref_recipe_round_trip_preserves_uuid() {
        let mut src = World::new();
        let uuid = AssetUUID::new_v5("meshes/teapot.gltf");
        let entity = src.spawn(MeshRef::Asset(uuid));

        let reg = inventory::iter::<ComponentRegistration>
            .into_iter()
            .find(|r| r.type_name == "MeshRef")
            .expect("MeshRef registration present");
        let bytes = (reg.serialize_recipe)(&src, entity).expect("serialize");

        let mut dst = World::new();
        let new_entity = dst.spawn(());
        (reg.deserialize_recipe)(&mut dst, new_entity, &bytes).expect("deserialize");

        let restored = dst.get::<MeshRef>(new_entity).expect("mesh ref restored");
        assert_eq!(restored, &MeshRef::Asset(uuid));
    }
}
