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
//! A widget is a free function `fn(ui: &mut dyn UiBuilder, theme: &UiTheme,
//! rect: Rect, …data) -> Interaction?`. It reads its colours from the
//! [`UiTheme`] it is handed, never from [`crate::brand::pal`]; paints through
//! [`UiBuilder`] primitives only, never `egui`; holds no state, the caller
//! owning it; and paints into the absolute rect it is given (from
//! [`UiBuilder::allocate_size`](khora_core::ui::UiBuilder::allocate_size), or
//! computed by the caller).
//!
//! A new widget is composed here from existing primitives: [`UiBuilder`] is
//! the backend contract and grows only for a genuinely new primitive. Test it
//! with [`crate::testing::RecordingUiBuilder`], asserting on theme slots and
//! what was painted, not on pixel coordinates.
//!
//! [`UiTheme`]: khora_core::ui::UiTheme
//! [`UiBuilder`]: khora_core::ui::UiBuilder

use khora_core::ui::UiTheme;

pub mod buttons;
pub mod chips;
pub mod containers;
pub mod dialog;
pub mod disclosure;
pub mod dock;
pub mod feedback;
pub mod fields;
pub mod film;
pub mod indicators;
pub mod loading;
pub mod mark;
pub mod measure;
pub mod modal;
pub mod nav;
pub mod paint;
pub mod scroll;

pub use buttons::{button, icon_button, Button, ButtonKind};
pub use chips::{chip, filter_pill, kbd_chip, Pill, Tone};
pub use containers::{card, card_action_slot};
pub use dialog::{dialog, wrap_text, Dialog, DialogChoice};
pub use disclosure::{group_header, Group};
pub use dock::{
    dock_drag_ghost, dock_drop_overlay, dock_splitter, dock_tab_strip, TabStripEvent,
    DOCK_DRAG_TAG, TAB_STRIP_H,
};
pub use feedback::{banner, empty_state, tooltip};
pub use fields::{
    axis_field, checkbox, error_line, field_label, input_frame, property_row, search_field, Axis,
};
pub use indicators::{
    health_bar, meter_bar, progress_row, skeleton, sparkline, status_dot, Health,
};
pub use loading::{LoadingScreen, LoadingState};
pub use mark::{brand_pill, sign, SIGN_NAVY, SIGN_SILVER};
pub use modal::{confirm_modal, Confirm, ModalChoice};
pub use nav::{
    breadcrumb, nav_item, panel_tab, radio_card, segmented_tabs, selectable_row, step_rail,
    StepState,
};
pub use paint::{fill, lerp_color, stroke, tint, vertical_gradient, with_alpha};
pub use scroll::{scrollbar, ScrollState};

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

/// Returns the inverse ink, for text on a filled `primary` (silver) surface.
///
/// Used by primary buttons and any other control filled with the brand colour.
#[inline]
pub fn on_primary(theme: &UiTheme) -> Color {
    theme.text_inverse
}
