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

//! Scrolling for panels that paint in absolute coordinates.
//!
//! The editor's panels compute their own rects and paint straight into window
//! space, which is why none of them scrolled: a stock scroll area expects to
//! own the layout, and adopting one would mean rewriting every panel to paint
//! relatively.
//!
//! Instead a panel keeps a [`ScrollState`], subtracts its offset from its own
//! `y` cursor, and clips to the viewport. Three lines per panel, no rewrite.

use khora_core::ui::{UiBuilder, UiTheme};

use super::paint::{fill, tint};
use super::Rect;

/// Width of the scrollbar track.
const BAR_W: f32 = 6.0;
/// Width of the grab area around the track. Wider than the paint so the bar can
/// stay slim without being fiddly to hit.
const HIT_W: f32 = 14.0;
/// Shortest the thumb is allowed to get, so a very long list still leaves
/// something grabbable.
const MIN_THUMB: f32 = 24.0;

/// Scroll offset for one panel, persisted across frames by the panel itself.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollState {
    /// Points scrolled down from the top. Always `>= 0`.
    offset: f32,
}

impl ScrollState {
    /// How far down the content is scrolled.
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// Scrolls back to the top — call when the content is replaced wholesale
    /// (a new scene, a changed filter) so the user isn't left staring at blank
    /// space below the end of a now-shorter list.
    pub fn reset(&mut self) {
        self.offset = 0.0;
    }

    /// Consumes the wheel over `viewport` and clamps the offset to
    /// `content_height`.
    ///
    /// Clamping every frame — rather than only when scrolling — is what keeps
    /// the view valid when the content *shrinks* underneath it.
    pub fn update(&mut self, ui: &dyn UiBuilder, viewport: Rect, content_height: f32) {
        self.offset = (self.offset - ui.scroll_delta_in(viewport))
            .clamp(0.0, Self::max_offset(viewport, content_height));
    }

    /// Scrolls so the thumb centres on `pointer_y`.
    ///
    /// Centring rather than tracking a grab-anchor means a click anywhere on
    /// the track jumps there, and a drag follows the cursor exactly — the
    /// behaviour that needs no explanation.
    pub fn drag_to(&mut self, pointer_y: f32, viewport: Rect, content_height: f32) {
        let [_, y, _, h] = viewport;
        let thumb_h = Self::thumb_height(viewport, content_height);
        let travel = h - thumb_h;
        if travel <= 0.0 {
            return;
        }
        let t = ((pointer_y - y - thumb_h * 0.5) / travel).clamp(0.0, 1.0);
        self.offset = t * Self::max_offset(viewport, content_height);
    }

    fn max_offset(viewport: Rect, content_height: f32) -> f32 {
        (content_height - viewport[3]).max(0.0)
    }

    fn thumb_height(viewport: Rect, content_height: f32) -> f32 {
        if content_height <= 0.0 {
            return viewport[3];
        }
        let visible_frac = (viewport[3] / content_height).clamp(0.0, 1.0);
        (viewport[3] * visible_frac)
            .max(MIN_THUMB)
            .min(viewport[3])
    }

    /// Whether the content overflows its viewport.
    pub fn overflows(&self, viewport: Rect, content_height: f32) -> bool {
        content_height > viewport[3] + 0.5
    }
}

/// Paints a scrollbar down the right edge of `viewport` and lets the user drag
/// it, if there is anything to scroll.
///
/// Drawn only on overflow: a permanent track on a panel that fits is chrome
/// telling the user about a constraint they do not have.
///
/// The hit area is wider than the painted bar — a 6 px target is precise mouse
/// work for something people expect to grab casually.
pub fn scrollbar(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    viewport: Rect,
    content_height: f32,
    state: &mut ScrollState,
    id_salt: &str,
) {
    if !state.overflows(viewport, content_height) || content_height <= 0.0 {
        return;
    }
    let [x, y, w, h] = viewport;
    let bar_x = x + w - BAR_W - 2.0;

    let hit = ui.interact_rect(
        id_salt,
        [bar_x - (HIT_W - BAR_W) * 0.5, y, HIT_W, h],
    );
    if hit.pressed {
        if let Some(p) = ui.pointer_position() {
            state.drag_to(p[1], viewport, content_height);
        }
    }

    let thumb_h = ScrollState::thumb_height(viewport, content_height);
    let travel = h - thumb_h;
    let max_offset = ScrollState::max_offset(viewport, content_height).max(1.0);
    let thumb_y = y + (state.offset / max_offset).clamp(0.0, 1.0) * travel;

    let thumb_colour = if hit.pressed {
        theme.accent_c
    } else if hit.hovered {
        theme.primary_dim
    } else {
        theme.border_strong
    };

    fill(ui, [bar_x, y, BAR_W, h], tint(theme.separator, 0.5), 3.0);
    fill(ui, [bar_x, thumb_y, BAR_W, thumb_h], thumb_colour, 3.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingUiBuilder;

    const VIEWPORT: Rect = [0.0, 0.0, 200.0, 100.0];

    fn theme() -> UiTheme {
        crate::khora_dark()
    }

    #[test]
    fn content_that_fits_does_not_scroll_or_paint_a_bar() {
        let mut state = ScrollState::default();
        let mut ui = RecordingUiBuilder::new(200.0, 100.0);
        state.update(&ui, VIEWPORT, 80.0);
        assert_eq!(state.offset(), 0.0);
        assert!(!state.overflows(VIEWPORT, 80.0));

        let before = ui.rects_filled().len();
        scrollbar(&mut ui, &theme(), VIEWPORT, 80.0, &mut state, "s");
        assert_eq!(ui.rects_filled().len(), before, "no bar when nothing to scroll");
    }

    #[test]
    fn overflowing_content_paints_a_track_and_a_thumb() {
        let mut state = ScrollState::default();
        let mut ui = RecordingUiBuilder::new(200.0, 100.0);
        scrollbar(&mut ui, &theme(), VIEWPORT, 400.0, &mut state, "s");
        assert_eq!(ui.rects_filled().len(), 2, "track + thumb");
    }

    /// Dragging the thumb has to move the view — the wheel is not the only way
    /// people scroll, and a bar that looks grabbable but isn't reads as broken.
    #[test]
    fn dragging_the_thumb_scrolls() {
        let mut state = ScrollState::default();
        // Press halfway down the track of a 100pt viewport over 300pt of
        // content.
        let mut ui = RecordingUiBuilder::new(200.0, 100.0)
            .with_press("bar")
            .with_pointer([196.0, 50.0]);
        scrollbar(&mut ui, &theme(), VIEWPORT, 300.0, &mut state, "bar");
        assert!(
            state.offset() > 0.0,
            "a press on the track must move the view, got {}",
            state.offset()
        );
    }

    /// The thumb centres on the cursor, so grabbing it and dragging to the
    /// bottom lands at the end of the content rather than short of it.
    #[test]
    fn dragging_to_the_bottom_reaches_the_end() {
        let mut state = ScrollState::default();
        state.drag_to(1000.0, VIEWPORT, 300.0);
        assert_eq!(state.offset(), 200.0);

        state.drag_to(-1000.0, VIEWPORT, 300.0);
        assert_eq!(state.offset(), 0.0);
    }

    /// The offset can never exceed what there is to scroll, or the panel would
    /// show blank space past the end of its own content.
    #[test]
    fn offset_clamps_to_the_scrollable_range() {
        let mut state = ScrollState::default();
        let ui = RecordingUiBuilder::new(200.0, 100.0).with_scroll(-10_000.0);
        state.update(&ui, VIEWPORT, 300.0);
        assert_eq!(state.offset(), 200.0, "300 of content in 100 of viewport");

        let ui = RecordingUiBuilder::new(200.0, 100.0).with_scroll(10_000.0);
        state.update(&ui, VIEWPORT, 300.0);
        assert_eq!(state.offset(), 0.0, "cannot scroll above the top");
    }

    /// Content shrinking under a scrolled view must pull it back up — the
    /// reason the clamp runs every frame rather than only on a wheel event.
    #[test]
    fn shrinking_content_pulls_the_view_back() {
        let mut state = ScrollState::default();
        let ui = RecordingUiBuilder::new(200.0, 100.0).with_scroll(-10_000.0);
        state.update(&ui, VIEWPORT, 500.0);
        assert_eq!(state.offset(), 400.0);

        // The list got shorter (a filter was applied, entries were dropped).
        let ui = RecordingUiBuilder::new(200.0, 100.0);
        state.update(&ui, VIEWPORT, 150.0);
        assert_eq!(state.offset(), 50.0, "clamped to the new content height");
    }
}
