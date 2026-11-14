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

//! The colour target's aspect, as a camera reads it.

use super::*;
use crate::math::Extent2D;

fn size(width: u32, height: u32) -> ColorTargetSize {
    ColorTargetSize(Extent2D { width, height })
}

/// The aspect of a target is its width over its height: a 1024×768
/// window is 4:3.
#[test]
fn a_color_target_aspect_is_width_over_height() {
    assert_eq!(size(1024, 768).aspect(), 4.0 / 3.0);
    assert_eq!(size(1920, 1080).aspect(), 16.0 / 9.0);
    assert_eq!(size(800, 800).aspect(), 1.0);
    assert_eq!(size(768, 1024).aspect(), 0.75);
}

/// A target with no width or no height (a minimised window) has no
/// shape to follow; its aspect is 1, never 0, infinite or NaN.
#[test]
fn a_color_target_with_a_zero_side_has_aspect_one() {
    assert_eq!(size(0, 768).aspect(), 1.0);
    assert_eq!(size(1024, 0).aspect(), 1.0);
    assert_eq!(size(0, 0).aspect(), 1.0);
}
