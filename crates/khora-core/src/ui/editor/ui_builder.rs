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

//! Abstract widget builder for editor panels.
//!
//! [`UiBuilder`] provides a backend-agnostic API for constructing immediate-mode
//! UI. Concrete implementations (e.g. `EguiUiBuilder` in `khora-infra`) translate
//! each call into the underlying UI library.

use super::viewport_texture::ViewportTextureHandle;

/// A backend-agnostic, immediate-mode widget builder.
///
/// Panels receive a `&mut dyn UiBuilder` and use it to draw headings, labels,
/// buttons, separators, and nested layouts without knowing which UI library is
/// behind the trait.
pub trait UiBuilder {
    // ── Text ───────────────────────────────────────────

    /// Large heading text.
    fn heading(&mut self, text: &str);

    /// Normal label text.
    fn label(&mut self, text: &str);

    /// Colored label.
    fn colored_label(&mut self, color: [f32; 4], text: &str);

    /// Small / secondary label.
    fn small_label(&mut self, text: &str);

    /// Monospaced text (for code / log output).
    fn monospace(&mut self, text: &str);

    // ── Interactive ────────────────────────────────────

    /// Push-button. Returns `true` the frame it is clicked.
    fn button(&mut self, text: &str) -> bool;

    /// Small push-button (less padding).
    fn small_button(&mut self, text: &str) -> bool;

    /// Selectable label — highlights when `active` is true.
    /// Returns `true` when clicked.
    fn selectable_label(&mut self, active: bool, text: &str) -> bool;

    /// Boolean checkbox. Returns `true` when toggled.
    fn checkbox(&mut self, checked: &mut bool, text: &str) -> bool;

    /// Draggable `f32` value. Returns `true` when changed.
    fn drag_value_f32(&mut self, label: &str, value: &mut f32, speed: f32) -> bool;

    /// Slider for `f32`. Returns `true` when changed.
    fn slider_f32(
        &mut self,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
    ) -> bool;

    /// Single-line text input. Returns `true` when changed.
    fn text_edit_singleline(&mut self, text: &mut String) -> bool;

    /// Editable Vec3 as three drag-values (X / Y / Z). Returns `true` when any component changed.
    fn vec3_editor(&mut self, label: &str, value: &mut [f32; 3], speed: f32) -> bool;

    /// RGBA color picker. Returns `true` when changed.
    fn color_edit(&mut self, label: &str, color: &mut [f32; 4]) -> bool;

    /// Drop-down combo box picking among string options.
    /// `current` is the index of the currently selected item.
    /// Returns `true` when the selection changed.
    fn combo_box(&mut self, label: &str, current: &mut usize, options: &[&str]) -> bool;

    // ── Layout ─────────────────────────────────────────

    /// Horizontal layout — children placed left-to-right.
    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Vertical layout (default, but useful inside a horizontal).
    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Collapsible section with a header.
    fn collapsing(
        &mut self,
        header: &str,
        default_open: bool,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    );

    /// Indented block.
    fn indent(&mut self, id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Scrollable area.
    fn scroll_area(&mut self, id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder));

    // ── Decoration ─────────────────────────────────────

    /// Horizontal separator line.
    fn separator(&mut self);

    /// Blank vertical spacing (in logical points).
    fn spacing(&mut self, points: f32);

    // ── Interaction ────────────────────────────────────

    /// Returns `true` if the last widget was double-clicked.
    fn is_last_item_double_clicked(&self) -> bool;

    /// Returns `true` if the last widget is currently hovered by the pointer.
    fn is_last_item_hovered(&self) -> bool;

    /// Shows a right-click context menu on the last widget.
    /// The closure is called to build menu content when the menu is open.
    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    // ── Queries ────────────────────────────────────────

    /// Available width in the current layout region.
    fn available_width(&self) -> f32;

    /// Available height in the current layout region.
    fn available_height(&self) -> f32;

    // ── Viewport / Images ──────────────────────────────

    /// Display a viewport texture at the given size.
    ///
    /// Returns the top-left position `[x, y]` of the rendered image in
    /// window-space pixels (useful for hit-testing / picking).
    /// Returns `None` if the backend cannot display this handle.
    fn viewport_image(
        &mut self,
        handle: ViewportTextureHandle,
        size: [f32; 2],
    ) -> Option<[f32; 2]>;
}
