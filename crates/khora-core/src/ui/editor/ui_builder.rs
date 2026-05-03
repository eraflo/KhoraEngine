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

/// Result of an [`UiBuilder::interact_rect`] call.
#[derive(Debug, Clone, Copy, Default)]
pub struct Interaction {
    /// Pointer is inside the rect this frame.
    pub hovered: bool,
    /// Primary button was pressed inside the rect this frame.
    pub clicked: bool,
    /// Primary button is currently held inside the rect.
    pub pressed: bool,
    /// Pointer double-clicked inside the rect this frame.
    pub double_clicked: bool,
}

/// Font family hint passed to [`UiBuilder::paint_text_styled`]. Backends map
/// each variant to whichever face was registered for that family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamilyHint {
    /// Default proportional / sans-serif (Geist if installed).
    Proportional,
    /// Monospaced (Geist Mono if installed).
    Monospace,
    /// Icon font (Lucide if installed). Pass single-char codepoints.
    Icons,
}

/// Horizontal alignment for [`UiBuilder::paint_text_styled`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    /// Anchor at the left edge.
    Left,
    /// Anchor at the centre.
    Center,
    /// Anchor at the right edge.
    Right,
}

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

    /// Selectable label that returns `true` when double-clicked.
    fn selectable_label_double_clicked(&mut self, active: bool, text: &str) -> bool;

    /// Boolean checkbox. Returns `true` when toggled.
    fn checkbox(&mut self, checked: &mut bool, text: &str) -> bool;

    /// Draggable `f32` value. Returns `true` when changed.
    fn drag_value_f32(&mut self, label: &str, value: &mut f32, speed: f32) -> bool;

    /// Slider for `f32`. Returns `true` when changed.
    fn slider_f32(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool;

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

    /// Returns `true` if Enter was pressed while the last widget had focus.
    fn is_last_item_enter_pressed(&self) -> bool;

    /// Returns `true` if Escape was pressed while the last widget had focus.
    fn is_last_item_escape_pressed(&self) -> bool;

    /// Shows a right-click context menu on the last widget.
    /// The closure is called to build menu content when the menu is open.
    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Shows a right-click context menu when the user right-clicks anywhere on
    /// the current panel background (not on a specific widget).
    ///
    /// Allocates an invisible full-width/height region to detect right-clicks.
    fn context_menu_panel(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        // Default no-op. Backends that support it override this method.
        let _ = f;
    }

    /// Close the currently open context menu (if any).
    ///
    /// Call this after a menu action has been executed so the popup dismisses
    /// and the action takes effect on the same frame.
    fn close_menu(&mut self) {
        // Default no-op.
    }

    /// Shows a sub-menu button inside a context menu.
    /// Unlike `collapsing`, this creates a proper egui sub-menu that doesn't
    /// steal focus from the parent context menu.
    fn menu_button(&mut self, label: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (label, f);
    }

    // ── Painting / Overlays ───────────────────────────

    /// Paints a line in window-space coordinates.
    fn paint_line(&mut self, from: [f32; 2], to: [f32; 2], color: [f32; 4], thickness: f32) {
        let _ = (from, to, color, thickness);
    }

    /// Paints a filled rectangle in window-space coordinates.
    fn paint_rect_filled(&mut self, min: [f32; 2], size: [f32; 2], color: [f32; 4], rounding: f32) {
        let _ = (min, size, color, rounding);
    }

    /// Paints text at a window-space position.
    fn paint_text(&mut self, pos: [f32; 2], color: [f32; 4], text: &str) {
        let _ = (pos, color, text);
    }

    /// Paints a stroked (outlined) rectangle in window-space.
    fn paint_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        let _ = (min, size, color, rounding, thickness);
    }

    /// Paints a filled circle.
    fn paint_circle_filled(&mut self, center: [f32; 2], radius: f32, color: [f32; 4]) {
        let _ = (center, radius, color);
    }

    /// Paints a circle outline.
    fn paint_circle_stroke(
        &mut self,
        center: [f32; 2],
        radius: f32,
        color: [f32; 4],
        thickness: f32,
    ) {
        let _ = (center, radius, color, thickness);
    }

    /// Paints text with explicit size, font family and alignment.
    fn paint_text_styled(
        &mut self,
        pos: [f32; 2],
        text: &str,
        size: f32,
        color: [f32; 4],
        family: FontFamilyHint,
        align: TextAlign,
    ) {
        let _ = (pos, text, size, color, family, align);
    }

    /// Paints a closed polygon path (for diamonds, triangles, custom shapes).
    /// `points` is a list of `[x, y]` window-space coordinates.
    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        let _ = (points, color);
    }

    /// Allocates a clickable region at the given absolute window-space rect
    /// and reports interaction this frame. The `id_salt` disambiguates
    /// overlapping or repeatedly-painted hot regions.
    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        let _ = (id_salt, rect);
        Interaction::default()
    }

    /// Attaches a drag payload (typically an entity ID packed into a
    /// `u64`) to the **last interacted region** (the most recent
    /// [`interact_rect`](Self::interact_rect) call). Should be called
    /// immediately after `interact_rect`. Avoids creating a competing
    /// hit-target on the same rect, which would steal pointer events.
    /// Default: no-op.
    fn dnd_attach_drag_payload(&mut self, payload: u64) {
        let _ = payload;
    }

    /// If a drag-and-drop just released on the **last interacted region**,
    /// returns its `u64` payload. Should be called after
    /// [`interact_rect`](Self::interact_rect). Default: no-op.
    fn dnd_take_drop_payload(&mut self) -> Option<u64> {
        None
    }

    /// Attaches a tooltip to the most recently created widget / interaction.
    fn tooltip_for_last(&mut self, text: &str) {
        let _ = text;
    }

    /// Pushes a child layout region at the given absolute screen-space rect.
    /// Inside the closure, `&mut dyn UiBuilder` reflects the constrained
    /// region — egui-native widgets (`button`, `text_edit_singleline`,
    /// `vec3_editor`, …) lay out within it instead of the parent panel.
    /// Used by composite widgets (inspector cards) that paint their frame
    /// absolutely but want native egui controls inside.
    fn region_at(&mut self, rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        // Default fallback for backends that don't support sub-regions:
        // do nothing. Concrete backends (egui) MUST override this — calling
        // it on a backend without an override silently no-ops the body.
        let _ = (rect, f);
    }

    /// Returns the current layout cursor in screen-space `(x, y)`. Useful for
    /// composites that need to know where to place an absolutely-painted
    /// frame before advancing egui's natural layout.
    fn cursor_pos(&self) -> [f32; 2] {
        let r = self.panel_rect();
        [r[0], r[1]]
    }

    /// Measures the rendered size of `text` at `size` points using `family`.
    /// Returns `[width, height]` in logical points.
    ///
    /// Backends without a real text shaper fall back to a heuristic
    /// (~0.55 × size per character, height = size). The egui backend uses
    /// the actual font metrics — call this rather than guessing widths.
    fn measure_text(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        let _ = family;
        // Default heuristic: ~0.55 average glyph aspect ratio.
        let w = text.chars().count() as f32 * size * 0.55;
        [w, size]
    }

    // ── Queries ────────────────────────────────────────

    /// Available width in the current layout region.
    fn available_width(&self) -> f32;

    /// Available height in the current layout region.
    fn available_height(&self) -> f32;

    /// Returns the current paint region in *screen-space* coordinates as
    /// `[min_x, min_y, width, height]`.
    ///
    /// Useful for panels that need to draw custom chrome (rounded backgrounds,
    /// gradients, branded pills) on top of their content. The returned
    /// rectangle matches the area covered by the current layout, including
    /// space already consumed by widgets.
    ///
    /// Default implementation falls back to a `(0, 0)` origin and the
    /// available size — backends should override it to return their real
    /// region.
    fn panel_rect(&self) -> [f32; 4] {
        [0.0, 0.0, self.available_width(), self.available_height()]
    }

    /// Returns the full screen size (top-left = `(0, 0)`) as
    /// `[min_x, min_y, width, height]`.
    ///
    /// Used by floating overlays / modals that need to cover or center
    /// themselves on the entire viewport. Default implementation falls back
    /// to [`panel_rect`](Self::panel_rect).
    fn screen_rect(&self) -> [f32; 4] {
        self.panel_rect()
    }

    // ── Viewport / Images ──────────────────────────────

    /// Display a viewport texture at the given size.
    ///
    /// Returns the top-left position `[x, y]` of the rendered image in
    /// window-space pixels (useful for hit-testing / picking).
    /// Returns `None` if the backend cannot display this handle.
    fn viewport_image(&mut self, handle: ViewportTextureHandle, size: [f32; 2])
        -> Option<[f32; 2]>;
}
