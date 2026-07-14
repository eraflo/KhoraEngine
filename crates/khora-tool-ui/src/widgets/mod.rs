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

//! The shared widget vocabulary — one look for every Khora tool.
//!
//! # The contract
//!
//! Every widget in this module is a **free function** with the shape:
//!
//! ```text
//! fn widget(ui: &mut dyn UiBuilder, theme: &UiTheme, rect: Rect, …data) -> Interaction?
//! ```
//!
//! and obeys four rules. They are what make the system extensible:
//!
//! 1. **Theme in, never `pal` out.** A widget reads its colors from the
//!    [`UiTheme`] it is handed — never from [`crate::brand::pal`] directly.
//!    That is the whole reason the editor and the hub cannot drift, and why
//!    re-theming a tool is just passing a different theme.
//! 2. **No backend.** Widgets paint through [`UiBuilder`] primitives only. No
//!    `egui` type ever appears here, so the day the backend changes, none of
//!    this moves.
//! 3. **No state.** The caller owns the state (which tab is open, which pills
//!    are enabled); the widget takes it by value or `&mut` and returns what
//!    happened. Widgets are pure functions of their inputs.
//! 4. **Absolute rects.** A widget paints into the `[x, y, w, h]` it is given.
//!    Callers using auto-layout get one from
//!    [`UiBuilder::allocate_size`]; callers doing manual layout (most editor
//!    panels) compute it themselves.
//!
//! # Adding a widget
//!
//! Put composites here as free functions. **Do not add methods to the
//! [`UiBuilder`] trait** — that trait is the backend contract and only grows
//! when a genuinely new *primitive* (a paint or interaction capability that
//! cannot be composed from the existing ones) is needed. Every widget below is
//! built from the primitives that already exist.
//!
//! Test with [`crate::testing::RecordingUiBuilder`]: assert on which theme slot
//! was used and what was painted, not on pixel coordinates.
//!
//! [`UiTheme`]: khora_core::ui::UiTheme
//! [`UiBuilder`]: khora_core::ui::UiBuilder

use khora_core::ui::UiTheme;

pub mod buttons;
pub mod chips;
pub mod containers;
pub mod disclosure;
pub mod feedback;
pub mod fields;
pub mod indicators;
pub mod mark;
pub mod nav;
pub mod paint;

pub use buttons::{button, icon_button, Button, ButtonKind};
pub use chips::{chip, filter_pill, kbd_chip, Pill, Tone};
pub use containers::{card, card_action_slot};
pub use disclosure::{group_header, Group};
pub use feedback::{banner, empty_state, tooltip};
pub use fields::{
    axis_field, checkbox, error_line, field_label, input_frame, property_row, search_field, Axis,
};
pub use indicators::{
    health_bar, meter_bar, progress_row, skeleton, sparkline, status_dot, Health,
};
pub use mark::{brand_pill, diamond};
pub use nav::{
    breadcrumb, nav_item, panel_tab, radio_card, segmented_tabs, selectable_row, step_rail,
    StepState,
};
pub use paint::{fill, lerp_color, stroke, tint, vertical_gradient, with_alpha};

/// A rectangle as `[x, y, width, height]` — the same shape
/// [`UiBuilder::interact_rect`] and [`UiBuilder::allocate_size`] speak.
///
/// [`UiBuilder::interact_rect`]: khora_core::ui::UiBuilder::interact_rect
/// [`UiBuilder::allocate_size`]: khora_core::ui::UiBuilder::allocate_size
pub type Rect = [f32; 4];

/// An RGBA color as `[r, g, b, a]`, sRGB in `0..=1` (see [`crate::brand`]).
pub type Color = [f32; 4];

/// Right edge of a rect.
#[inline]
pub fn right(r: Rect) -> f32 {
    r[0] + r[2]
}

/// Bottom edge of a rect.
#[inline]
pub fn bottom(r: Rect) -> f32 {
    r[1] + r[3]
}

/// Centre point of a rect.
#[inline]
pub fn center(r: Rect) -> [f32; 2] {
    [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]
}

/// Shrinks a rect by `d` on every side.
#[inline]
pub fn shrink(r: Rect, d: f32) -> Rect {
    [
        r[0] + d,
        r[1] + d,
        (r[2] - 2.0 * d).max(0.0),
        (r[3] - 2.0 * d).max(0.0),
    ]
}

/// The baseline `y` that vertically centres text of `size` inside `rect`.
///
/// Text is anchored at its top edge by [`UiBuilder::paint_text_styled`], so
/// this returns the top, not the typographic baseline.
///
/// [`UiBuilder::paint_text_styled`]: khora_core::ui::UiBuilder::paint_text_styled
#[inline]
pub fn text_y(rect: Rect, size: f32) -> f32 {
    rect[1] + (rect[3] - size) * 0.5
}

/// Resolves the foreground color for text drawn on top of a filled `primary`
/// (silver) surface — the inverse ink. Used by primary buttons and any other
/// control that fills with the brand color.
#[inline]
pub fn on_primary(theme: &UiTheme) -> Color {
    theme.text_inverse
}
