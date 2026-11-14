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

//! A radial gradient for the paint contract.

use super::gradient::{color_at_offset, GradientStop};

/// A radial gradient in window-space coordinates: offset 0 at `center`,
/// offset 1 on the circle of `radius` around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadialGradient<'a> {
    /// Where offset 0 lies.
    pub center: [f32; 2],
    /// Distance from `center` at which offset 1 lies.
    pub radius: f32,
    /// The colour stops, sorted by offset.
    pub stops: &'a [GradientStop],
}

impl RadialGradient<'_> {
    /// Distance of `p` from `center` divided by `radius`: 0 at the centre, 1
    /// on the circle; not clamped. A radius ≤ 0 gives 1.0.
    pub fn offset_at(&self, p: [f32; 2]) -> f32 {
        if self.radius <= 0.0 {
            return 1.0;
        }
        let d = [p[0] - self.center[0], p[1] - self.center[1]];
        (d[0] * d[0] + d[1] * d[1]).sqrt() / self.radius
    }

    /// The colour at `p`, with the same stop semantics as
    /// [`LinearGradient::color_at`](super::LinearGradient::color_at): clamped
    /// to the first / last stop outside them, linear (componentwise) between
    /// stops. No stops gives `[0.0; 4]`, one stop gives its colour, and a
    /// radius ≤ 0 gives the last stop's colour.
    pub fn color_at(&self, p: [f32; 2]) -> [f32; 4] {
        color_at_offset(self.stops, self.offset_at(p))
    }
}
