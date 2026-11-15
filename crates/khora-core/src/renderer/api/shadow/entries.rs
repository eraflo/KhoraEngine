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

//! Per-light shadow entries that shadow strategies write into the
//! [`ShadowFrame`](super::ShadowFrame) deck slot and lit lanes read by light index.

use std::collections::HashMap;

use crate::math::{Mat4, Vec3};

/// Shadow data computed by a shadow strategy for a single light.
///
/// One variant per atlas: a 2D depth array for directional and spot lights, a
/// depth cube array for point lights.
#[derive(Debug, Clone)]
pub enum ShadowEntry {
    /// Directional / spot shadow stored in the 2D depth-array atlas.
    Atlas2D {
        /// Light's view-projection matrix used to sample the shadow atlas.
        view_proj: Mat4,
        /// Layer index inside the 2D atlas.
        atlas_index: i32,
    },
    /// Point-light omnidirectional shadow stored in the cubemap atlas.
    Cube {
        /// Per-face view-projection matrices in [`crate::math::CubeFace::ALL`]
        /// order (`[+X, -X, +Y, -Y, +Z, -Z]`), boxed to keep the enum small.
        face_view_projs: Box<[Mat4; 6]>,
        /// Layer index inside the cube-array atlas; face `f` is layer
        /// `cube_array_index * 6 + f`.
        cube_array_index: i32,
        /// World-space light position, from which the shader samples the cube.
        light_pos: Vec3,
        /// Light's range, the `far` plane of the six face projections.
        ///
        /// The shader rebuilds the non-linear depth from it to compare.
        far_plane: f32,
    },
}

/// Per-frame shadow lookup keyed by light index in `RenderWorld.lights`.
#[derive(Debug, Default, Clone)]
pub struct ShadowEntries(pub HashMap<usize, ShadowEntry>);

impl ShadowEntries {
    /// Inserts (or replaces) shadow data for the light at `light_index`.
    pub fn insert(&mut self, light_index: usize, entry: ShadowEntry) {
        self.0.insert(light_index, entry);
    }

    /// Looks up shadow data for the light at `light_index`.
    pub fn get(&self, light_index: usize) -> Option<&ShadowEntry> {
        self.0.get(&light_index)
    }

    /// Returns the number of shadow entries recorded.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether no shadow entry is recorded.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_atlas2d_entry() {
        let mut entries = ShadowEntries::default();
        entries.insert(
            3,
            ShadowEntry::Atlas2D {
                view_proj: Mat4::IDENTITY,
                atlas_index: 1,
            },
        );
        match entries.get(3) {
            Some(ShadowEntry::Atlas2D { atlas_index, .. }) => assert_eq!(*atlas_index, 1),
            other => panic!("expected Atlas2D, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_cube_entry() {
        let mut entries = ShadowEntries::default();
        entries.insert(
            5,
            ShadowEntry::Cube {
                face_view_projs: Box::new([Mat4::IDENTITY; 6]),
                cube_array_index: 2,
                light_pos: Vec3::new(1.0, 2.0, 3.0),
                far_plane: 50.0,
            },
        );
        match entries.get(5) {
            Some(ShadowEntry::Cube {
                cube_array_index,
                light_pos,
                far_plane,
                ..
            }) => {
                assert_eq!(*cube_array_index, 2);
                assert_eq!(*light_pos, Vec3::new(1.0, 2.0, 3.0));
                assert_eq!(*far_plane, 50.0);
            }
            other => panic!("expected Cube, got {other:?}"),
        }
    }
}
