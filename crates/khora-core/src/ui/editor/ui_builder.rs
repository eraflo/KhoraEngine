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

    /// Draws large heading text.
    fn heading(&mut self, text: &str);

    /// Draws normal label text.
    fn label(&mut self, text: &str);

    /// Draws a label in `color`.
    fn colored_label(&mut self, color: [f32; 4], text: &str);

    /// Draws a small, secondary label.
    fn small_label(&mut self, text: &str);

    /// Draws monospaced text, for code and log output.
    fn monospace(&mut self, text: &str);

    // ── Interactive ────────────────────────────────────

    /// Draws a push-button; returns `true` the frame it is clicked.
    fn button(&mut self, text: &str) -> bool;

    /// Draws a push-button with less padding; returns `true` the frame it is clicked.
    fn small_button(&mut self, text: &str) -> bool;

    /// Draws a selectable label, highlighted when `active`; returns `true` when clicked.
    fn selectable_label(&mut self, active: bool, text: &str) -> bool;

    /// Draws a selectable label; returns `true` when double-clicked.
    fn selectable_label_double_clicked(&mut self, active: bool, text: &str) -> bool;

    /// Draws a checkbox; returns `true` when toggled.
    fn checkbox(&mut self, checked: &mut bool, text: &str) -> bool;

    /// Draws a draggable `f32` value; returns `true` when changed.
    fn drag_value_f32(&mut self, label: &str, value: &mut f32, speed: f32) -> bool;

    /// Draws a draggable `f32` value kept between `min` and `max` once edited.
    ///
    /// Returns `true` when changed. A value already outside the bounds is
    /// shown and kept as it is until the author edits it.
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

    /// Draws a label in `color` that says `tooltip` when hovered.
    ///
    /// The default draws the label alone.
    fn colored_label_with_tooltip(&mut self, color: [f32; 4], text: &str, tooltip: &str) {
        let _ = tooltip;
        self.colored_label(color, text);
    }

    /// Draws an `f32` slider; returns `true` when changed.
    fn slider_f32(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool;

    /// Draws a single-line text input; returns `true` when changed.
    fn text_edit_singleline(&mut self, text: &mut String) -> bool;

    /// Draws a Vec3 as three drag-values (X / Y / Z); returns `true` when any changed.
    fn vec3_editor(&mut self, label: &str, value: &mut [f32; 3], speed: f32) -> bool;

    /// Draws an RGBA color picker; returns `true` when changed.
    fn color_edit(&mut self, label: &str, color: &mut [f32; 4]) -> bool;

    /// Draws a dropdown over `options`, writing the picked index into `current`.
    ///
    /// Returns `true` when the selection changed. `id_salt` must be stable
    /// across frames and unique among sibling combo boxes: the backend keys the
    /// popup's open state on it, and the inspector labels every enum alike.
    fn combo_box(
        &mut self,
        id_salt: &str,
        label: &str,
        current: &mut usize,
        options: &[&str],
    ) -> bool;

    // ── Layout ─────────────────────────────────────────

    /// Lays out the children `f` draws left to right.
    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Lays out the children `f` draws top to bottom, as inside a horizontal layout.
    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Draws a collapsible section with a header.
    fn collapsing(
        &mut self,
        header: &str,
        default_open: bool,
        f: &mut dyn FnMut(&mut dyn UiBuilder),
    );

    /// Draws an indented block.
    fn indent(&mut self, id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Draws a scrollable area.
    fn scroll_area(&mut self, id: &str, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Draws an inset panel in the top `height` pixels of the current region.
    ///
    /// The rest of the region is left for later calls, typically a
    /// [`central_inset`](Self::central_inset): a header bar nested inside a
    /// screen. Does nothing by default.
    fn top_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, height, f);
    }

    /// Draws an inset panel along the bottom edge.
    ///
    /// The counterpart of [`top_inset_panel`](Self::top_inset_panel).
    fn bottom_inset_panel(&mut self, id: &str, height: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, height, f);
    }

    /// Draws an inset sidebar in the left `width` pixels of the current region.
    fn left_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, width, f);
    }

    /// Draws an inset sidebar along the right edge.
    ///
    /// The counterpart of [`left_inset_panel`](Self::left_inset_panel).
    fn right_inset_panel(&mut self, id: &str, width: f32, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, width, f);
    }

    /// Draws into the space left once the inset panels are placed.
    fn central_inset(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = f;
    }

    /// Wraps `f` in a visual frame with an optional fill, a stroke and rounded corners.
    ///
    /// `f` draws inside the frame, `margin` in from its edge.
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

    /// Opens a centered modal dialog of logical `size` over the current screen.
    ///
    /// The rest of the screen is dimmed; `f` draws the dialog's interior. Does
    /// nothing by default.
    fn modal(&mut self, id: &str, size: [f32; 2], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id, size, f);
    }

    // ── Decoration ─────────────────────────────────────

    /// Draws a horizontal separator line.
    fn separator(&mut self);

    /// Adds blank vertical spacing, in logical points.
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

    /// Returns `true` if the last interacted region is being dragged.
    ///
    /// Drag sources paint a cursor-following ghost by it. Default: `false`.
    fn is_last_item_dragged(&self) -> bool {
        false
    }

    /// Returns whether `key` was pressed this frame **while no text field takes keyboard input**.
    ///
    /// So a shortcut never eats keystrokes meant for a field. Default: `false`,
    /// a backend without keyboard support reporting no shortcuts.
    fn key_pressed(&self, key: KeyCode) -> bool {
        let _ = key;
        false
    }

    /// Returns whether any widget holds keyboard focus, typically a text field.
    ///
    /// Panels use it to suppress their own single-key shortcuts. Default:
    /// `false`.
    fn keyboard_captured(&self) -> bool {
        false
    }

    /// Returns whether `key` was pressed this frame, **regardless of focus**.
    ///
    /// For the widget that *owns* the focused field, such as a command palette
    /// moving its selection while the user types its query. Prefer
    /// [`key_pressed`] everywhere else.
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

    /// Returns the scroll-wheel delta this frame while the pointer is inside `rect`, in points.
    ///
    /// Positive moves the content **down** (scrolled towards the top). Scoped
    /// to a rect so two scrollable panels never consume one gesture. Default: `0.0`.
    fn scroll_delta_in(&self, rect: [f32; 4]) -> f32 {
        let _ = rect;
        0.0
    }

    /// Shows a right-click context menu on the last widget, built by `f` while open.
    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Shows a context menu on a right-click on the current panel's background.
    ///
    /// Does nothing by default.
    fn context_menu_panel(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = f;
    }

    /// Closes the open context menu, if any.
    ///
    /// Call it after running a menu action, so the popup closes on the same frame.
    fn close_menu(&mut self) {}

    /// Shows a sub-menu button inside a context menu.
    ///
    /// Unlike `collapsing`, the sub-menu does not steal focus from its parent menu.
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

    /// Paints a closed polygon through the window-space `[x, y]` `points`.
    fn paint_path_filled(&mut self, points: &[[f32; 2]], color: [f32; 4]) {
        let _ = (points, color);
    }

    /// Paints a closed convex polygon through window-space `points`, filled with a linear gradient.
    ///
    /// The default paints one flat fill in the gradient's colour at the vertex
    /// centroid. Fewer than 3 points paint nothing.
    fn paint_path_filled_gradient(&mut self, points: &[[f32; 2]], gradient: LinearGradient<'_>) {
        if let Some(c) = flat_fill_point(points) {
            self.paint_path_filled(points, gradient.color_at(c));
        }
    }

    /// Paints a closed convex polygon filled with a radial gradient.
    ///
    /// The default paints one flat fill at the centroid colour; fewer than 3
    /// points paint nothing.
    fn paint_path_filled_radial(&mut self, points: &[[f32; 2]], gradient: RadialGradient<'_>) {
        if let Some(c) = flat_fill_point(points) {
            self.paint_path_filled(points, gradient.color_at(c));
        }
    }

    /// Allocates a clickable region at a window-space rect and reports this frame's interaction.
    ///
    /// `id_salt` tells overlapping or repeatedly painted regions apart.
    fn interact_rect(&mut self, id_salt: &str, rect: [f32; 4]) -> Interaction {
        let _ = (id_salt, rect);
        Interaction::default()
    }

    /// Attaches a `u64` drag payload, such as an entity id, to the **last interacted region**.
    ///
    /// Call it right after [`interact_rect`](Self::interact_rect): it adds no
    /// competing hit-target that would steal pointer events. Does nothing by default.
    fn dnd_attach_drag_payload(&mut self, payload: u64) {
        let _ = payload;
    }

    /// Returns the `u64` payload of a drag just dropped on the **last interacted region**.
    ///
    /// Call it after [`interact_rect`](Self::interact_rect). Default: `None`.
    fn dnd_take_drop_payload(&mut self) -> Option<u64> {
        None
    }

    /// Returns the `u64` payload of a drag hovering the **last interacted region**.
    ///
    /// Read but not taken: a drop there still delivers it. Default: `None`.
    fn dnd_hover_payload(&self) -> Option<u64> {
        None
    }

    /// Shows a menu on a primary click on the last widget or interacted region.
    ///
    /// It closes on a pick or a click outside. Does nothing by default.
    fn menu_on_click_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = f;
    }

    /// Returns the pointer position in window-space `[x, y]`, `None` outside the window.
    ///
    /// Drop targets place a dropped item where the cursor released by it.
    /// Default: `None`.
    fn pointer_position(&self) -> Option<[f32; 2]> {
        None
    }

    /// Returns `true` while a `u64` drag payload is in flight this frame.
    ///
    /// Drop targets show a "droppable here" highlight by it. Default: `false`.
    fn is_drag_active(&self) -> bool {
        false
    }

    /// Draws a single-line text editor at a window-space rect and returns this frame's outcome.
    ///
    /// Detects Enter, Escape and focus loss on its own. Pass `request_focus`
    /// on the frame the field first appears so the user can type at once.
    /// Default: [`InlineEditEvent::Idle`].
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

    /// Paints a filled rounded rect in an **unclipped overlay layer** above all panels.
    ///
    /// For drag ghosts and other marks that must show outside the current
    /// panel's clip rect. Does nothing by default.
    fn overlay_rect_filled(
        &mut self,
        min: [f32; 2],
        size: [f32; 2],
        color: [f32; 4],
        rounding: f32,
    ) {
        let _ = (min, size, color, rounding);
    }

    /// Paints a stroked rounded rect in the unclipped overlay layer.
    ///
    /// See [`overlay_rect_filled`](Self::overlay_rect_filled). Does nothing by default.
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

    /// Paints text in the unclipped overlay layer.
    ///
    /// See [`overlay_rect_filled`](Self::overlay_rect_filled). Does nothing by default.
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

    /// Lays out the widgets `f` draws within a screen-space rect instead of the parent panel.
    ///
    /// `id_salt` must be **stable across frames and unique within the parent**:
    /// the region's widget ids, which carry focus and text-edit state, derive
    /// from it. Does nothing by default, `f` included.
    fn region_at(&mut self, id_salt: &str, rect: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let _ = (id_salt, rect, f);
    }

    /// Returns the current layout cursor in screen-space `[x, y]`.
    ///
    /// Where a composite places an absolutely painted frame before the layout advances.
    fn cursor_pos(&self) -> [f32; 2] {
        let r = self.panel_rect();
        [r[0], r[1]]
    }

    /// Reserves `[width, height]` at the current cursor and returns the rect as `[x, y, w, h]`.
    ///
    /// Unlike [`interact_rect`](Self::interact_rect), the space is **reserved in
    /// the parent layout**, so a custom-painted widget keeps later widgets off
    /// it. The default reserves nothing and returns the rect at `cursor_pos()`.
    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        let p = self.cursor_pos();
        [p[0], p[1], size[0], size[1]]
    }

    /// Returns the `[width, height]` of `text` at `size` points in `family`, in logical points.
    ///
    /// The default estimates 0.55 × `size` per character and a height of `size`.
    fn measure_text(&self, text: &str, size: f32, family: FontFamilyHint) -> [f32; 2] {
        let _ = family;
        let w = text.chars().count() as f32 * size * 0.55;
        [w, size]
    }

    // ── Queries ────────────────────────────────────────

    /// Returns the available width in the current layout region.
    fn available_width(&self) -> f32;

    /// Returns the available height in the current layout region.
    fn available_height(&self) -> f32;

    /// Returns the current paint region in *screen-space* as `[min_x, min_y, width, height]`.
    ///
    /// The area of the current layout, space already taken by widgets included.
    /// The default returns a `(0, 0)` origin and the available size.
    fn panel_rect(&self) -> [f32; 4] {
        [0.0, 0.0, self.available_width(), self.available_height()]
    }

    /// Returns the full screen as `[min_x, min_y, width, height]`, its top-left at `(0, 0)`.
    ///
    /// For overlays and modals covering or centering on the whole viewport.
    /// The default returns [`panel_rect`](Self::panel_rect).
    fn screen_rect(&self) -> [f32; 4] {
        self.panel_rect()
    }

    /// Returns the physical pixels per UI point.
    ///
    /// Converts this builder's sizes and positions (points) to window pixels,
    /// the unit of the platform cursor and render targets. Default: `1.0`.
    fn pixels_per_point(&self) -> f32 {
        1.0
    }

    // ── Viewport / Images ──────────────────────────────

    /// Displays a viewport texture at `size` and returns the image's window-space top-left.
    ///
    /// The position serves hit-testing and picking. `None` if the backend
    /// cannot display this handle.
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
