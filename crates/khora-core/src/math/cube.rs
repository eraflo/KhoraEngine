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

//! The six faces of a cube, in the order wgpu indexes a `texture_cube`, with each face's
//! camera axes: what point-light shadows, reflection probes and IBL render through.

use crate::math::Vec3;

/// One of the six faces of an axis-aligned cube.
///
/// The discriminants are stable (`u8`) and match the wgpu cubemap face
/// ordering, so [`CubeFace::index`] can be used directly to address a
/// `texture_cube_array` layer set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CubeFace {
    /// `+X` face.
    PosX = 0,
    /// `-X` face.
    NegX = 1,
    /// `+Y` face.
    PosY = 2,
    /// `-Y` face.
    NegY = 3,
    /// `+Z` face.
    PosZ = 4,
    /// `-Z` face.
    NegZ = 5,
}

impl CubeFace {
    /// All six faces in wgpu cubemap order: `[+X, -X, +Y, -Y, +Z, -Z]`.
    pub const ALL: [CubeFace; 6] = [
        CubeFace::PosX,
        CubeFace::NegX,
        CubeFace::PosY,
        CubeFace::NegY,
        CubeFace::PosZ,
        CubeFace::NegZ,
    ];

    /// Returns the layer index this face occupies inside a
    /// `texture_cube_array` slice (`0..=5`).
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Returns the world-space direction the camera looks toward when rasterising this face.
    #[inline]
    pub const fn forward(self) -> Vec3 {
        match self {
            CubeFace::PosX => Vec3::X,
            CubeFace::NegX => Vec3::NEG_X,
            CubeFace::PosY => Vec3::Y,
            CubeFace::NegY => Vec3::NEG_Y,
            CubeFace::PosZ => Vec3::Z,
            CubeFace::NegZ => Vec3::NEG_Z,
        }
    }

    /// Returns the face camera's world-space up, in the wgpu / Vulkan / D3D11 cubemap convention.
    ///
    /// The ±Y faces use ±Z, so `look_at_rh(eye, eye + face.forward(), face.up())` is never
    /// degenerate.
    #[inline]
    pub const fn up(self) -> Vec3 {
        match self {
            CubeFace::PosX | CubeFace::NegX => Vec3::NEG_Y,
            CubeFace::PosY => Vec3::Z,
            CubeFace::NegY => Vec3::NEG_Z,
            CubeFace::PosZ | CubeFace::NegZ => Vec3::NEG_Y,
        }
    }

    /// Returns the face whose [`forward`](Self::forward) axis is most aligned with `dir`.
    ///
    /// The largest absolute component wins (X before Y before Z on ties), its sign picks the
    /// half-axis: the major-axis rule the GPU applies when sampling a cube.
    pub fn from_direction(dir: Vec3) -> CubeFace {
        let abs = dir.abs();
        if abs.x >= abs.y && abs.x >= abs.z {
            if dir.x >= 0.0 {
                CubeFace::PosX
            } else {
                CubeFace::NegX
            }
        } else if abs.y >= abs.z {
            if dir.y >= 0.0 {
                CubeFace::PosY
            } else {
                CubeFace::NegY
            }
        } else if dir.z >= 0.0 {
            CubeFace::PosZ
        } else {
            CubeFace::NegZ
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::EPSILON;

    #[test]
    fn all_faces_iterate_in_wgpu_order() {
        let names: Vec<&str> = CubeFace::ALL
            .iter()
            .map(|f| match f {
                CubeFace::PosX => "+X",
                CubeFace::NegX => "-X",
                CubeFace::PosY => "+Y",
                CubeFace::NegY => "-Y",
                CubeFace::PosZ => "+Z",
                CubeFace::NegZ => "-Z",
            })
            .collect();
        assert_eq!(names, vec!["+X", "-X", "+Y", "-Y", "+Z", "-Z"]);
    }

    #[test]
    fn face_indices_match_wgpu_convention() {
        assert_eq!(CubeFace::PosX.index(), 0);
        assert_eq!(CubeFace::NegX.index(), 1);
        assert_eq!(CubeFace::PosY.index(), 2);
        assert_eq!(CubeFace::NegY.index(), 3);
        assert_eq!(CubeFace::PosZ.index(), 4);
        assert_eq!(CubeFace::NegZ.index(), 5);
        // The ALL array's position must also match.
        for (i, face) in CubeFace::ALL.iter().enumerate() {
            assert_eq!(face.index(), i);
        }
    }

    #[test]
    fn forward_and_up_are_orthogonal() {
        for face in CubeFace::ALL {
            let f = face.forward();
            let u = face.up();
            assert!(
                f.dot(u).abs() < EPSILON,
                "face {face:?} has non-orthogonal forward/up: dot = {}",
                f.dot(u)
            );
            // Both must be unit length.
            assert!((f.length() - 1.0).abs() < EPSILON);
            assert!((u.length() - 1.0).abs() < EPSILON);
        }
    }

    #[test]
    fn from_direction_round_trips_with_forward() {
        for face in CubeFace::ALL {
            assert_eq!(CubeFace::from_direction(face.forward()), face);
        }
    }

    #[test]
    fn from_direction_picks_major_axis() {
        // Slightly off-axis vectors still pick the dominant face.
        assert_eq!(
            CubeFace::from_direction(Vec3::new(0.9, 0.1, 0.05)),
            CubeFace::PosX
        );
        assert_eq!(
            CubeFace::from_direction(Vec3::new(-0.9, 0.05, -0.1)),
            CubeFace::NegX
        );
        assert_eq!(
            CubeFace::from_direction(Vec3::new(0.1, 0.9, 0.05)),
            CubeFace::PosY
        );
        assert_eq!(
            CubeFace::from_direction(Vec3::new(0.05, -0.9, 0.1)),
            CubeFace::NegY
        );
        assert_eq!(
            CubeFace::from_direction(Vec3::new(0.1, 0.05, 0.9)),
            CubeFace::PosZ
        );
        assert_eq!(
            CubeFace::from_direction(Vec3::new(0.05, 0.1, -0.9)),
            CubeFace::NegZ
        );
    }

    #[test]
    fn forward_directions_cover_all_six_axes() {
        // Sanity: the six forward vectors are pairwise distinct.
        let mut seen = std::collections::HashSet::new();
        for face in CubeFace::ALL {
            let f = face.forward();
            // Encode as fixed-point so HashSet works on f32.
            let key = (
                (f.x * 1_000_000.0) as i64,
                (f.y * 1_000_000.0) as i64,
                (f.z * 1_000_000.0) as i64,
            );
            assert!(seen.insert(key), "duplicate forward direction for {face:?}");
        }
        assert_eq!(seen.len(), 6);
    }
}
