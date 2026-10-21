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

//! [`PaintCall`] — one call a [`RecordingUiBuilder`](super::RecordingUiBuilder) recorded.

use khora_core::ui::editor::ui_builder::{FontFamilyHint, TextAlign};
use khora_core::ui::editor::GradientStop;

/// One recorded paint or interaction call, in the order the widget made it.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintCall {
    /// A filled rectangle.
    RectFilled {
        /// Top-left corner.
        min: [f32; 2],
        /// Width / height.
        size: [f32; 2],
        /// Fill color.
        color: [f32; 4],
        /// Corner radius.
        rounding: f32,
    },
    /// A stroked (outlined) rectangle.
    RectStroke {
        /// Top-left corner.
        min: [f32; 2],
        /// Width / height.
        size: [f32; 2],
        /// Stroke color.
        color: [f32; 4],
        /// Corner radius.
        rounding: f32,
        /// Stroke width.
        thickness: f32,
    },
    /// A straight line.
    Line {
        /// Start point.
        from: [f32; 2],
        /// End point.
        to: [f32; 2],
        /// Stroke color.
        color: [f32; 4],
        /// Stroke width.
        thickness: f32,
    },
    /// A filled circle.
    CircleFilled {
        /// Centre point.
        center: [f32; 2],
        /// Radius.
        radius: f32,
        /// Fill color.
        color: [f32; 4],
    },
    /// A stroked circle.
    CircleStroke {
        /// Centre point.
        center: [f32; 2],
        /// Radius.
        radius: f32,
        /// Stroke color.
        color: [f32; 4],
        /// Stroke width.
        thickness: f32,
    },
    /// A run of text.
    Text {
        /// Anchor position.
        pos: [f32; 2],
        /// The string painted.
        text: String,
        /// Font size in points.
        size: f32,
        /// Text color.
        color: [f32; 4],
        /// Which font family was requested.
        family: FontFamilyHint,
        /// Horizontal anchoring.
        align: TextAlign,
    },
    /// A filled polygon.
    Path {
        /// Polygon vertices.
        points: Vec<[f32; 2]>,
        /// Fill color.
        color: [f32; 4],
    },
    /// A polygon filled with a linear gradient.
    PathGradient {
        /// Polygon vertices.
        points: Vec<[f32; 2]>,
        /// Where the gradient's offset 0 lies.
        from: [f32; 2],
        /// Where the gradient's offset 1 lies.
        to: [f32; 2],
        /// The gradient's colour stops.
        stops: Vec<GradientStop>,
    },
    /// A polygon filled with a radial gradient.
    PathRadial {
        /// Polygon vertices.
        points: Vec<[f32; 2]>,
        /// Where the gradient's offset 0 lies.
        center: [f32; 2],
        /// Distance from `center` at which the gradient's offset 1 lies.
        radius: f32,
        /// The gradient's colour stops.
        stops: Vec<GradientStop>,
    },
    /// A hot region allocated for interaction.
    Interact {
        /// The disambiguating salt the widget passed.
        id_salt: String,
        /// The rect `[x, y, w, h]` made interactive.
        rect: [f32; 4],
    },
}
