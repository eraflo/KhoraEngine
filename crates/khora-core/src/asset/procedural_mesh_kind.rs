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

//! [`ProceduralMeshKind`], the primitives the engine generates.

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

impl ProceduralMeshKind {
    /// Returns the primitive's stable name: `Cube`, `Sphere` or `Plane`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cube => "Cube",
            Self::Sphere => "Sphere",
            Self::Plane => "Plane",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProceduralMeshKind;

    /// Each primitive has its own name.
    #[test]
    fn each_primitive_names_itself() {
        assert_eq!(ProceduralMeshKind::Cube.name(), "Cube");
        assert_eq!(ProceduralMeshKind::Sphere.name(), "Sphere");
        assert_eq!(ProceduralMeshKind::Plane.name(), "Plane");
    }
}
