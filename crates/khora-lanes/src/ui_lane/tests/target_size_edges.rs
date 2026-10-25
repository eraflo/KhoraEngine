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

//! The UI projection at the extremes of the colour target's size.

use super::super::ui_projection;
use khora_core::math::{Extent2D, Mat4, Vec4};

fn ndc(projection: &Mat4, x: f32, y: f32) -> (f32, f32) {
    let clip = *projection * Vec4::new(x, y, 0.0, 1.0);
    (clip.x / clip.w, clip.y / clip.w)
}

fn assert_corners(target: Extent2D) {
    let projection = ui_projection(target).expect("a target with area has a projection");
    let (w, h) = (target.width as f32, target.height as f32);
    for ((x, y), expected) in [
        ((0.0, 0.0), (-1.0, 1.0)),
        ((w, h), (1.0, -1.0)),
        ((w, 0.0), (1.0, 1.0)),
        ((0.0, h), (-1.0, -1.0)),
    ] {
        let got = ndc(&projection, x, y);
        let close = (got.0 - expected.0).abs() < 1e-5 && (got.1 - expected.1).abs() < 1e-5;
        assert!(
            close,
            "{target:?}: pixel ({x}, {y}) -> {got:?}, expected {expected:?}"
        );
    }
    assert!(
        projection
            .to_cols_array_2d()
            .iter()
            .flatten()
            .all(|v| v.is_finite()),
        "{target:?}: {projection:?}"
    );
}

#[test]
fn a_one_pixel_target_projects_its_pixel_onto_the_whole_clip_space() {
    assert_corners(Extent2D {
        width: 1,
        height: 1,
    });
}

#[test]
fn the_largest_targets_still_project_their_corners_onto_clip_space() {
    for target in [
        Extent2D {
            width: 16_384,
            height: 16_384,
        },
        Extent2D {
            width: 32_768,
            height: 1,
        },
        Extent2D {
            width: u32::MAX,
            height: u32::MAX,
        },
    ] {
        assert_corners(target);
    }
}
