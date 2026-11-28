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

//! [`ProceduralMesh`], a mesh a reference carries as a primitive and its parameters.

use super::{AssetUUID, ContentHasher, LocalSource, ProceduralMeshKind};
use crate::renderer::api::gpu_scene::{create_cube, create_plane, create_sphere, Mesh};

/// A primitive and its parameters, named by its content.
///
/// Equal primitives share one uuid, hence one copy in the asset store and on
/// a device; nothing else can name it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProceduralMesh {
    kind: ProceduralMeshKind,
    params: [f32; 4],
    uuid: AssetUUID,
}

impl ProceduralMesh {
    /// Returns the primitive `kind` with `params`, its uuid derived from both.
    ///
    /// Unused parameter slots are zero: cube `[size]`, plane `[size, y]`,
    /// sphere `[radius, segments, rings]`.
    pub fn new(kind: ProceduralMeshKind, params: [f32; 4]) -> Self {
        let mut hasher = ContentHasher::new();
        hasher.write_str("ProceduralMesh");
        hasher.write_str(kind.name());
        for param in params {
            hasher.write_f32(param);
        }
        Self {
            kind,
            params,
            uuid: hasher.uuid(),
        }
    }

    /// Returns a cube of side 1, the mesh an entity is given by default.
    pub fn unit_cube() -> Self {
        Self::new(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0])
    }

    /// Returns the primitive generated.
    pub fn kind(&self) -> ProceduralMeshKind {
        self.kind
    }

    /// Returns the primitive's parameters, unused slots zero.
    pub fn params(&self) -> [f32; 4] {
        self.params
    }
}

impl LocalSource<Mesh> for ProceduralMesh {
    const VARIANT: &'static str = "Procedural";

    fn uuid(&self) -> AssetUUID {
        self.uuid
    }

    fn build(&self) -> Mesh {
        let [a, b, c, _] = self.params;
        match self.kind {
            ProceduralMeshKind::Cube => create_cube(a),
            ProceduralMeshKind::Plane => create_plane(a, b),
            ProceduralMeshKind::Sphere => create_sphere(a, b as u32, c as u32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Equal parameters give one uuid; other parameters or another primitive another.
    #[test]
    fn a_procedural_mesh_is_named_by_its_content() {
        let sphere = |segments: f32| {
            ProceduralMesh::new(ProceduralMeshKind::Sphere, [0.5, segments, 8.0, 0.0])
        };
        assert_eq!(sphere(16.0).uuid(), sphere(16.0).uuid());
        assert_ne!(sphere(16.0).uuid(), sphere(32.0).uuid());
        assert_ne!(
            ProceduralMesh::new(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0]).uuid(),
            ProceduralMesh::new(ProceduralMeshKind::Plane, [1.0, 0.0, 0.0, 0.0]).uuid()
        );
    }

    /// The unit cube is a cube of side 1, and keeps what it was built from.
    #[test]
    fn the_unit_cube_is_a_cube_of_side_one() {
        let cube = ProceduralMesh::unit_cube();
        assert_eq!(cube.kind(), ProceduralMeshKind::Cube);
        assert_eq!(cube.params(), [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(
            cube,
            ProceduralMesh::new(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0])
        );
    }

    /// Each primitive builds its own mesh from its parameters.
    #[test]
    fn a_procedural_mesh_builds_its_primitive() {
        let cube = ProceduralMesh::new(ProceduralMeshKind::Cube, [2.0, 0.0, 0.0, 0.0]).build();
        assert_eq!(cube.positions.len(), 24);
        let plane = ProceduralMesh::new(ProceduralMeshKind::Plane, [4.0, -1.0, 0.0, 0.0]).build();
        assert!(plane.positions.iter().all(|p| p.y == -1.0));
        let sphere = ProceduralMesh::new(ProceduralMeshKind::Sphere, [1.0, 8.0, 4.0, 0.0]).build();
        assert_eq!(sphere.positions.len(), 9 * 5);
    }
}
