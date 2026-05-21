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

//! Per-light shadow entries — abstract types every shadow strategy
//! emits into the [`ShadowFrame`](super::ShadowFrame) deck slot for
//! lit consumer lanes to look up by light index.
//!
//! Lives in `khora-core` because both producers (shadow lanes) and
//! consumers (lit lanes) speak this contract.

use std::collections::HashMap;

use crate::math::{Mat4, Vec3};

/// Shadow data computed by a shadow strategy for a single light.
///
/// Two variants reflect the two atlas binding surfaces:
///
/// - [`ShadowEntry::Atlas2D`] — directional / spot, sampled with one
///   view-projection matrix against a `texture_depth_2d_array`.
/// - [`ShadowEntry::Cube`] — point lights, sampled directionally
///   against a `texture_depth_cube_array`. The CPU-side per-face
///   matrices are kept on the entry so the strategy can rebuild bind
///   data without re-running its source flow.
///
/// `Cube` is boxed to keep the enum size close to `Atlas2D` (most
/// lights in a typical scene are directional / spot).
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
        /// Per-face view-projection matrices in
        /// [`crate::math::CubeFace::ALL`] order
        /// (`[+X, -X, +Y, -Y, +Z, -Z]`).
        face_view_projs: Box<[Mat4; 6]>,
        /// Layer index inside the cube-array atlas (the GPU consumes
        /// `cube_array_index * 6 + face_index` to address a face).
        cube_array_index: i32,
        /// World-space light position. The shader uses this to compute
        /// the fragment-to-light direction it samples the cubemap with.
        light_pos: Vec3,
        /// Light's effective range — the perspective `far` plane used
        /// when rendering the six faces. The shader recomputes the
        /// non-linear depth value with this constant to compare against
        /// the sampled depth.
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

    /// Number of shadow entries currently recorded.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no shadow entries are currently recorded.
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
