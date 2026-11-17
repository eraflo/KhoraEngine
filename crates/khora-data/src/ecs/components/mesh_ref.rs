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

//! [`MeshRef`], the authored reference to a mesh, and its registration for
//! scenes, snapshots and the inspector.

use khora_core::asset::AssetUUID;

/// A procedural mesh primitive the engine can generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProceduralMeshKind {
    /// Axis-aligned cube primitive.
    Cube,
    /// UV sphere primitive.
    Sphere,
    /// Flat XZ plane primitive.
    Plane,
}

/// An authored reference to a mesh: the only form a mesh is saved in.
///
/// A resolver `DataSystem` (in `khora-io`) turns it into a `HandleComponent<Mesh>`,
/// and again whenever its [`uuid`](Self::uuid) differs from the handle's.
/// Build `Procedural` with [`MeshRef::procedural`], so its UUID matches its parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshRef {
    /// A primitive the resolver generates from `kind` and `params`.
    Procedural {
        /// Which primitive to generate.
        kind: ProceduralMeshKind,
        /// Generator parameters, unused slots zero: cube `[size]`, plane
        /// `[size, y]`, sphere `[radius, segments, rings]`.
        params: [f32; 4],
        /// The identity derived from `kind` and `params`; never saved, recomputed on load.
        uuid: AssetUUID,
    },
    /// An imported mesh (glTF, OBJ) in the VFS, by its stable UUID.
    Asset(AssetUUID),
}

impl crate::ecs::Component for MeshRef {}

impl MeshRef {
    /// Returns a `Procedural` reference whose UUID is derived from `kind` and `params`.
    ///
    /// Identical primitives share one UUID, hence one resolved handle and one
    /// `GpuMesh`: it is the content hash the GPU projection keys on.
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

    /// Returns the identity UUID: derived from the parameters for `Procedural`,
    /// the asset's for `Asset`.
    pub fn uuid(&self) -> AssetUUID {
        match self {
            Self::Procedural { uuid, .. } => *uuid,
            Self::Asset(uuid) => *uuid,
        }
    }

    /// Returns a unit cube, the default authored mesh.
    pub fn unit_cube() -> Self {
        Self::procedural(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0])
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

/// The saved form of a [`MeshRef`]: a procedural UUID is left out, recomputed on load.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename = "MeshRef")]
enum SerializableMeshRef {
    Procedural {
        kind: ProceduralMeshKind,
        params: [f32; 4],
    },
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

/// Writes a page row's `MeshRef` as a record.
fn mesh_ref_to_record(
    column: &dyn crate::ecs::AnyVec,
    row: usize,
    references: &mut dyn crate::scene::record::ReferenceWriter,
) -> Result<crate::scene::record::Record, crate::scene::record::RecordError> {
    let mesh_ref = <MeshRef as crate::ecs::Component>::clone_from_column(column, row);
    crate::scene::record::to_record(&SerializableMeshRef::from(&mesh_ref), references)
}

/// Returns the schema of a mesh reference's positional form, traced once.
fn mesh_ref_schema() -> (u64, bool) {
    static SCHEMA: std::sync::OnceLock<(u64, bool)> = std::sync::OnceLock::new();
    crate::scene::schema::traced_once::<SerializableMeshRef>(&SCHEMA, "MeshRef")
}

/// Reads a `MeshRef` back from a record, its procedural identity recomputed.
fn stage_mesh_ref(
    record: &crate::scene::record::Record,
    references: &mut dyn crate::scene::record::ReferenceReader,
) -> Result<crate::scene::StagedComponent, crate::scene::record::RecordError> {
    let (on_disk, report) =
        crate::scene::record::resolve::<SerializableMeshRef>(record, references)?;
    Ok(crate::scene::StagedComponent {
        component: Box::new(crate::scene::StagedValue(MeshRef::from(on_disk))),
        report,
    })
}

/// Returns the editor JSON form: the saved shape, without a procedural UUID.
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
        formerly: &[],
        resumable: false,
        column_to_record: mesh_ref_to_record,
        stage: stage_mesh_ref,
        schema: || mesh_ref_schema().0,
        schema_complete: || mesh_ref_schema().1,
        column_to_snapshot: |column, row, out, references| {
            let mesh_ref = <MeshRef as crate::ecs::Component>::clone_from_column(column, row);
            crate::scene::positional::to_positional(
                &SerializableMeshRef::from(&mesh_ref),
                out,
                references,
            )
        },
        stage_snapshot: |bytes, references| {
            let on_disk = crate::scene::positional::from_positional::<SerializableMeshRef>(
                bytes, references,
            )?;
            Ok(crate::scene::StagedComponent {
                component: Box::new(crate::scene::StagedValue(MeshRef::from(on_disk))),
                report: Vec::new(),
            })
        },
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
        // A one-of: no field a script reaches.
        script_type: |_| None,
        script_default: || None,
        check: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::World;
    use crate::scene::component_registration::ComponentRegistration;

    /// `MeshRef::Procedural` survives a record round trip.
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
        let mut dst = World::new();
        let new_entity = dst.spawn(());
        crate::scene::component_registration::copy_through_record(
            reg, &src, entity, &mut dst, new_entity,
        )
        .expect("the source carries a MeshRef")
        .expect("it round-trips");

        let restored = dst.get::<MeshRef>(new_entity).expect("mesh ref restored");
        // The recomputed identity UUID matches the original (content-derived).
        assert_eq!(
            restored,
            &MeshRef::procedural(ProceduralMeshKind::Sphere, [0.75, 32.0, 16.0, 0.0])
        );
    }

    /// `MeshRef::Asset(uuid)` round-trips with its UUID unchanged.
    #[test]
    fn asset_mesh_ref_recipe_round_trip_preserves_uuid() {
        let mut src = World::new();
        let uuid = AssetUUID::new_v5("meshes/teapot.gltf");
        let entity = src.spawn(MeshRef::Asset(uuid));

        let reg = inventory::iter::<ComponentRegistration>
            .into_iter()
            .find(|r| r.type_name == "MeshRef")
            .expect("MeshRef registration present");
        let mut dst = World::new();
        let new_entity = dst.spawn(());
        crate::scene::component_registration::copy_through_record(
            reg, &src, entity, &mut dst, new_entity,
        )
        .expect("the source carries a MeshRef")
        .expect("it round-trips");

        let restored = dst.get::<MeshRef>(new_entity).expect("mesh ref restored");
        assert_eq!(restored, &MeshRef::Asset(uuid));
    }
}
