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

use super::gradient::{flat_fill_point, LinearGradient, RadialGradient};
use super::viewport_texture::ViewportTextureHandle;
use crate::platform::input::KeyCode;

pub use super::font_family_hint::FontFamilyHint;
pub use super::inline_edit_event::InlineEditEvent;
pub use super::interaction::Interaction;
pub use super::text_align::TextAlign;

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

    /// Draggable `f32` value kept between `min` and `max` once edited.
    /// Returns `true` when changed. A value already outside the bounds is
    /// shown as it is, and stays as it is until the author edits it.
    ///
    /// The default draws [`drag_value_f32`](Self::drag_value_f32) and
    /// clamps the value only when it changed.
    fn drag_value_f32_in(
        &mut self,
        label: &str,
        value: &mut f32,
        speed: f32,
        min: f32,
        max: f32,
    ) -> bool {
        let changed = self.drag_value_f32(label, value, speed);
        if changed {
            // Never `f32::clamp`, which panics on reversed bounds.
            *value = value.max(min).min(max);
        }
        changed
    }

    /// A label in `color` that says `tooltip` when hovered. The default
    /// draws the label alone.
    fn colored_label_with_tooltip(&mut self, color: [f32; 4], text: &str, tooltip: &str) {
        let _ = tooltip;
        self.colored_label(color, text);
    }

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
    /// Dropdown over `options`, writing the picked index into `current`.
    ///
    /// `id_salt` must be stable across frames and unique among sibling combo
    /// boxes: the backend keys the popup's open state on it. Deriving it from
    /// `label` is not enough — the inspector's generic enum walker labels every
    /// switchable enum the same way.
    fn combo_box(
        &mut self,
        id_salt: &str,
        label: &str,
        current: &mut usize,
        options: &[&str],
    ) -> bool;

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

    /// Inset panel along the top edge of the current region.
    ///
    /// Splits the layout vertically: the closure draws into the top
    /// `height` pixels, then the rest of the region is left for
    /// subsequent calls (typically a [`central_inset`](Self::central_inset)).
    /// Used by tools that want a header bar nested inside a screen
    /// (the hub does this for its title bar).
    ///
    /// Default implementation is a no-op so the trait stays
    /// object-safe — the egui backend overrides it.
    fn top_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, height, f);
    }

    /// Inset panel along the bottom edge — counterpart of
    /// [`top_inset_panel`](Self::top_inset_panel).
    fn bottom_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, height, f);
    }

    /// Inset sidebar on the left edge of the current region.
    ///
    /// Splits the layout horizontally: the closure draws into the
    /// left `width` pixels.
    fn left_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, width, f);
    }

    /// Inset sidebar on the right edge — counterpart of
    /// [`left_inset_panel`](Self::left_inset_panel).
    fn right_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, width, f);
    }

    /// Central region — fills whatever space remains after the inset
    /// panels above were placed.
    fn central_inset(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = f;
    }

    /// Wraps `f` in a visual frame: paints the optional fill / stroke /
    /// rounded background first, then runs `f` inside the inner
    /// content rect (after applying `margin`).
    ///
    /// Replaces the old `egui::Frame::new().inner_margin(...).show(...)`
    /// idiom for tool apps that want decorative frames without
    /// importing a backend type.
    fn frame_box(
        &mut self,
        margin: crate::ui::Margin,
        fill: Option<crate::math::LinearRgba>,
        stroke: crate::ui::Stroke,
        radius: crate::ui::CornerRadius,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    ) {
        let _ = (margin, fill, stroke, radius, f);
    }

    /// Open a modal dialog overlaid on top of the current screen.
    ///
    /// The dialog is centered, has the requested logical `size`, and
    /// dims the rest of the screen with a semi-transparent backdrop
    /// to convey focus. The closure receives a `UiBuilder` scoped to
    /// the dialog's interior.
    ///
    /// Default impl is a no-op so the trait stays object-safe — the
    /// egui backend overrides.
    fn modal(&mut self, id: &str, size: [f32; 2], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, size, f);
    }

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

    /// Returns `true` if the last interacted region is currently being dragged.
    /// Used by drag sources to paint a cursor-following ghost so the user can
    /// see a drag is in progress. Default: `false`.
    fn is_last_item_dragged(&self) -> bool {
        false
    }

    /// Whether `key` was pressed this frame **and no text field is consuming
    /// keyboard input**.
    ///
    /// The focus condition is the whole point: a panel that drives selection
    /// with the arrow keys must go quiet while the user is typing in a search
    /// box, and every caller getting that right by hand is how a shortcut ends
    /// up eating keystrokes meant for a field.
    ///
    /// Default: `false`, so a backend without keyboard support simply reports
    /// no shortcuts rather than pretending.
    fn key_pressed(&self, key: KeyCode) -> bool {
        let _ = key;
        false
    }

    /// Whether any widget currently holds keyboard focus — typically a text
    /// field being typed into.
    ///
    /// Panels use it to suppress their own single-key shortcuts. Default:
    /// `false`.
    fn keyboard_captured(&self) -> bool {
        false
    }

    /// Whether `key` was pressed this frame, **regardless of focus**.
    ///
    /// For the widget that *owns* the focused field: a command palette must
    /// still move its selection with the arrows while the user types in its
    /// own query box, which is exactly the case [`key_pressed`] refuses to
    /// serve. Reach for that one by default; this is the deliberate exception.
    ///
    /// [`key_pressed`]: Self::key_pressed
    fn raw_key_pressed(&self, key: KeyCode) -> bool {
        let _ = key;
        false
    }

    /// Asks the backend to give the **last** widget keyboard focus.
    ///
    /// Call it on the frame a field appears, not every frame: repeating the
    /// request traps focus so the user can never tab away.
    fn focus_last_item(&mut self) {}

    /// Restricts painting to `rect` until the matching [`pop_clip_rect`].
    ///
    /// The editor's panels paint in absolute window coordinates, so a scrolled
    /// list has to be clipped explicitly — otherwise its rows draw straight
    /// over the neighbouring panels.
    ///
    /// [`pop_clip_rect`]: Self::pop_clip_rect
    fn push_clip_rect(&mut self, rect: [f32; 4]) {
        let _ = rect;
    }

    /// Restores the clip region saved by [`push_clip_rect`].
    ///
    /// [`push_clip_rect`]: Self::push_clip_rect
    fn pop_clip_rect(&mut self) {}

    /// Accumulated scroll-wheel delta this frame while the pointer is inside
    /// `rect`, in points. Positive means the content should move **down**
    /// (the user scrolled towards the top of the list).
    ///
    /// Scoped to a rect rather than reported globally so two scrollable panels
    /// on screen can't both consume the same gesture. Default: `0.0`.
    fn scroll_delta_in(&self, rect: [f32; 4]) -> f32 {
        let _ = rect;
        0.0
    }

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

    /// Paints a closed polygon path (triangles, the brand sign, custom shapes).
    /// `points` is a list of `[x, y]` window-space coordinates.
    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        let _ = (points, color);
    }

    /// Paints a closed convex polygon filled with a linear gradient.
    /// `points` is a list of `[x, y]` window-space coordinates.
    ///
    /// Default: one flat [`paint_path_filled`](Self::paint_path_filled) in the
    /// gradient's colour at the vertex centroid. Fewer than 3 points paint
    /// nothing.
    fn paint_path_filled_gradient(&mut self, points: &[[f32; 2]], gradient: LinearGradient<'_>) {
        if let Some(c) = flat_fill_point(points) {
            self.paint_path_filled(points, gradient.color_at(c));
        }
    }

    /// Paints a closed convex polygon filled with a radial gradient. Default:
    /// one flat fill at the centroid colour; fewer than 3 points paint nothing.
    fn paint_path_filled_radial(&mut self, points: &[[f32; 2]], gradient: RadialGradient<'_>) {
        if let Some(c) = flat_fill_point(points) {
            self.paint_path_filled(points, gradient.color_at(c));
        }
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

    /// The `u64` payload of a drag hovering the **last interacted region**,
    /// read but not taken: a drop there still delivers it. Default: `None`.
    fn dnd_hover_payload(&self) -> Option<u64> {
        None
    }

    /// A menu that opens on a primary click on the last widget or interacted
    /// region, and closes on a pick or a click outside. Default: no-op.
    fn menu_on_click_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = f;
    }

    /// Current pointer (cursor) position in window-space `[x, y]`, or `None`
    /// when the pointer is outside the window or unknown. Drop targets use this
    /// to place a dropped item where the cursor released (e.g. unproject the
    /// drop point into the 3D scene). Default: `None`.
    fn pointer_position(&self) -> Option<[f32; 2]> {
        None
    }

    /// `true` while any `u64` drag payload is in flight this frame (a drag
    /// started and hasn't been released). Drop targets use it to show a
    /// "droppable here" highlight. Default: `false`.
    fn is_drag_active(&self) -> bool {
        false
    }

    /// Draws a single-line text editor at an absolute window-space rect and
    /// returns its outcome **this frame** (see [`InlineEditEvent`]). Unlike the
    /// generic [`region_at`](Self::region_at) + response-tracking helpers, this
    /// is self-contained: it owns the widget's `Response`, so Enter / Escape /
    /// focus-loss are detected reliably. Pass `request_focus = true` on the
    /// frame the field first appears so the user can type immediately.
    /// Default: no-op returning [`InlineEditEvent::Idle`].
    fn inline_text_field(
        &mut self,
        rect: [f32; 4],
        id_salt: &str,
        text: &mut String,
        request_focus: bool,
    ) -> InlineEditEvent {
        let _ = (rect, id_salt, text, request_focus);
        InlineEditEvent::Idle
    }

    /// Paints a filled rounded rect in an **unclipped top overlay layer**
    /// (above all panels). For cursor-following affordances like drag ghosts
    /// that must remain visible outside the current panel's clip rect.
    /// Default: no-op.
    fn overlay_rect_filled(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
    ) {
        let _ = (min, size, color, rounding);
    }

    /// Stroked rounded rect in the unclipped overlay layer. See
    /// [`overlay_rect_filled`](Self::overlay_rect_filled). Default: no-op.
    fn overlay_rect_stroke(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
        thickness: f32,
    ) {
        let _ = (min, size, color, rounding, thickness);
    }

    /// Text in the unclipped overlay layer. See
    /// [`overlay_rect_filled`](Self::overlay_rect_filled). Default: no-op.
    fn overlay_text(
        &mut self,
        pos: [f32; 2],
        text: &str,
        size: f32,
        color: [f32; 4],
        family: FontFamilyHint,
    ) {
        let _ = (pos, text, size, color, family);
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
    ///
    /// `id_salt` must be **stable across frames and unique within the parent**.
    /// The backend derives the region's widget ids from it, and those ids are
    /// what carries keyboard focus and text-edit state: a salt that changes
    /// between frames makes a field being typed into lose focus mid-word, and
    /// two regions sharing a salt make their contents collide.
    fn region_at(&mut self, id_salt: &str, rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        // Default fallback for backends that don't support sub-regions:
        // do nothing. Concrete backends (egui) MUST override this — calling
        // it on a backend without an override silently no-ops the body.
        let _ = (id_salt, rect, f);
    }

    /// Returns the current layout cursor in screen-space `(x, y)`. Useful for
    /// composites that need to know where to place an absolutely-painted
    /// frame before advancing egui's natural layout.
    fn cursor_pos(&self) -> [f32; 2] {
        let r = self.panel_rect();
        [r[0], r[1]]
    }

    /// Allocates `[width, height]` of space at the current cursor in
    /// the active layout direction. Returns the resulting rect as
    /// `[x, y, w, h]` — feed it to the paint primitives.
    ///
    /// Unlike [`interact_rect`](Self::interact_rect), this **reserves
    /// space in the parent layout** so subsequent widgets don't paint
    /// on top. Use this when a custom widget paints with
    /// `paint_rect_filled` etc. so the layout flow stays correct (for
    /// example inside a `horizontal()` row where the row's height
    /// must reflect the tallest custom-painted widget).
    ///
    /// The returned rect's position matches what the cursor would have
    /// been just before the call. Default impl falls back to the
    /// current `cursor_pos()` + the requested size — backends that
    /// support real layout allocation must override.
    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        let p = self.cursor_pos();
        [p[0], p[1], size[0], size[1]]
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

    /// Physical pixels per UI point — the factor from every size and position
    /// this builder reports (points) to the window's pixels, the unit of the
    /// platform cursor and of render targets. Default: `1.0`.
    fn pixels_per_point(&self) -> f32 {
        1.0
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::editor::gradient::GradientStop;

    /// A builder that records only flat polygon fills, so the default
    /// gradient fill is observed through what it delegates to.
    #[derive(Default)]
    struct PathRecorder {
        paths: Vec<(Vec<[f32; 2]>, [f32; 4])>,
    }

    impl UiBuilder for PathRecorder {
        fn heading(&mut self, _: &str) {}
        fn label(&mut self, _: &str) {}
        fn colored_label(&mut self, _: [f32; 4], _: &str) {}
        fn small_label(&mut self, _: &str) {}
        fn monospace(&mut self, _: &str) {}
        fn button(&mut self, _: &str) -> bool {
            false
        }
        fn small_button(&mut self, _: &str) -> bool {
            false
        }
        fn selectable_label(&mut self, _: bool, _: &str) -> bool {
            false
        }
        fn selectable_label_double_clicked(&mut self, _: bool, _: &str) -> bool {
            false
        }
        fn checkbox(&mut self, _: &mut bool, _: &str) -> bool {
            false
        }
        fn drag_value_f32(&mut self, _: &str, _: &mut f32, _: f32) -> bool {
            false
        }
        fn slider_f32(&mut self, _: &str, _: &mut f32, _: f32, _: f32) -> bool {
            false
        }
        fn text_edit_singleline(&mut self, _: &mut String) -> bool {
            false
        }
        fn vec3_editor(&mut self, _: &str, _: &mut [f32; 3], _: f32) -> bool {
            false
        }
        fn color_edit(&mut self, _: &str, _: &mut [f32; 4]) -> bool {
            false
        }
        fn combo_box(&mut self, _: &str, _: &str, _: &mut usize, _: &[&str]) -> bool {
            false
        }
        fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
            f(self);
        }
        fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
            f(self);
        }
        fn collapsing(&mut self, _: &str, _: bool, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
            f(self);
        }
        fn indent(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
            f(self);
        }
        fn scroll_area(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
            f(self);
        }
        fn separator(&mut self) {}
        fn spacing(&mut self, _: f32) {}
        fn is_last_item_double_clicked(&self) -> bool {
            false
        }
        fn is_last_item_hovered(&self) -> bool {
            false
        }
        fn is_last_item_enter_pressed(&self) -> bool {
            false
        }
        fn is_last_item_escape_pressed(&self) -> bool {
            false
        }
        fn context_menu_last(&mut self, _: &mut dyn FnMut(&mut dyn UiBuilder)) {}
        fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
            self.paths.push((points.to_vec(), color));
        }
        fn available_width(&self) -> f32 {
            100.0
        }
        fn available_height(&self) -> f32 {
            100.0
        }
        fn viewport_image(&mut self, _: ViewportTextureHandle, _: [f32; 2]) -> Option<[f32; 2]> {
            None
        }
    }

    const RED_TO_BLUE: [GradientStop; 2] = [
        GradientStop {
            offset: 0.0,
            color: [1.0, 0.0, 0.0, 1.0],
        },
        GradientStop {
            offset: 1.0,
            color: [0.0, 0.0, 1.0, 1.0],
        },
    ];

    /// Red at x = 0, blue at x = 90.
    fn red_to_blue() -> LinearGradient<'static> {
        LinearGradient {
            from: [0.0, 0.0],
            to: [90.0, 0.0],
            stops: &RED_TO_BLUE,
        }
    }

    #[test]
    fn default_gradient_fill_paints_one_flat_path_at_the_centroid_colour() {
        let mut ui = PathRecorder::default();
        // Vertex centroid [30, 30]: a third of the way along the axis.
        let triangle = [[0.0, 0.0], [90.0, 0.0], [0.0, 90.0]];
        ui.paint_path_filled_gradient(&triangle, red_to_blue());

        assert_eq!(ui.paths.len(), 1, "one flat path, not a mesh");
        let (points, color) = &ui.paths[0];
        assert_eq!(
            points.as_slice(),
            &triangle,
            "the polygon is passed through"
        );
        let expected = [2.0 / 3.0, 0.0, 1.0 / 3.0, 1.0];
        for (i, (got, want)) in color.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "channel {i}: got {color:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn default_gradient_fill_uses_the_vertex_centroid_of_a_quad() {
        let mut ui = PathRecorder::default();
        // Vertex centroid [67.5, 10]: three quarters of the way along the axis.
        let quad = [[45.0, 0.0], [90.0, 0.0], [90.0, 20.0], [45.0, 20.0]];
        ui.paint_path_filled_gradient(&quad, red_to_blue());

        assert_eq!(ui.paths.len(), 1);
        let color = ui.paths[0].1;
        let expected = [0.25, 0.0, 0.75, 1.0];
        for (i, (got, want)) in color.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "channel {i}: got {color:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn default_gradient_fill_with_fewer_than_three_points_paints_nothing() {
        let mut ui = PathRecorder::default();
        ui.paint_path_filled_gradient(&[], red_to_blue());
        ui.paint_path_filled_gradient(&[[0.0, 0.0]], red_to_blue());
        ui.paint_path_filled_gradient(&[[0.0, 0.0], [90.0, 0.0]], red_to_blue());
        assert!(ui.paths.is_empty(), "painted {:?}", ui.paths);
    }

    /// Red at the origin, blue 100 units from it.
    fn red_to_blue_disc() -> RadialGradient<'static> {
        RadialGradient {
            center: [0.0, 0.0],
            radius: 100.0,
            stops: &RED_TO_BLUE,
        }
    }

    #[test]
    fn default_radial_fill_paints_one_flat_path_at_the_centroid_colour() {
        let mut ui = PathRecorder::default();
        // Vertex centroid [30, 40]: 50 units from the centre, half the radius.
        let triangle = [[0.0, 0.0], [90.0, 0.0], [0.0, 120.0]];
        ui.paint_path_filled_radial(&triangle, red_to_blue_disc());

        assert_eq!(ui.paths.len(), 1, "one flat path, not a mesh");
        let (points, color) = &ui.paths[0];
        assert_eq!(
            points.as_slice(),
            &triangle,
            "the polygon is passed through"
        );
        let expected = [0.5, 0.0, 0.5, 1.0];
        for (i, (got, want)) in color.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "channel {i}: got {color:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn default_radial_fill_uses_the_vertex_centroid_of_a_quad() {
        let mut ui = PathRecorder::default();
        // Vertex centroid [0, 75]: three quarters of the radius away, below
        // the centre — the distance counts, not the direction.
        let quad = [[-10.0, 70.0], [10.0, 70.0], [10.0, 80.0], [-10.0, 80.0]];
        ui.paint_path_filled_radial(&quad, red_to_blue_disc());

        assert_eq!(ui.paths.len(), 1);
        let color = ui.paths[0].1;
        let expected = [0.25, 0.0, 0.75, 1.0];
        for (i, (got, want)) in color.iter().zip(expected).enumerate() {
            assert!(
                (got - want).abs() < 1e-5,
                "channel {i}: got {color:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn default_radial_fill_with_fewer_than_three_points_paints_nothing() {
        let mut ui = PathRecorder::default();
        ui.paint_path_filled_radial(&[], red_to_blue_disc());
        ui.paint_path_filled_radial(&[[0.0, 0.0]], red_to_blue_disc());
        ui.paint_path_filled_radial(&[[0.0, 0.0], [90.0, 0.0]], red_to_blue_disc());
        assert!(ui.paths.is_empty(), "painted {:?}", ui.paths);
    }
}
