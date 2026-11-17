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

//! What the editor's "create" menus spawn.

/// An entity the editor's "create" menus spawn, by what it starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpawnShape {
    /// A transform and a name, nothing else.
    Empty,
    /// A unit cube with a matte material.
    Cube,
    /// A sphere with a matte material.
    Sphere,
    /// A ground plane with a matte material.
    Plane,
    /// A directional light.
    Light,
    /// A perspective camera.
    Camera,
}

impl SpawnShape {
    /// Every shape, in the order the menus list them.
    pub const ALL: [SpawnShape; 6] = [
        SpawnShape::Empty,
        SpawnShape::Cube,
        SpawnShape::Sphere,
        SpawnShape::Plane,
        SpawnShape::Light,
        SpawnShape::Camera,
    ];

    /// Returns the shape's name: its menu entry and the spawned entity's `Name`.
    pub const fn label(self) -> &'static str {
        match self {
            SpawnShape::Empty => "Empty",
            SpawnShape::Cube => "Cube",
            SpawnShape::Sphere => "Sphere",
            SpawnShape::Plane => "Plane",
            SpawnShape::Light => "Light",
            SpawnShape::Camera => "Camera",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SpawnShape;

    /// The labels are the names the menus show and the spawned entities take.
    #[test]
    fn each_shape_is_labelled_as_its_menu_entry() {
        let labels: Vec<_> = SpawnShape::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels,
            ["Empty", "Cube", "Sphere", "Plane", "Light", "Camera"]
        );
    }
}
