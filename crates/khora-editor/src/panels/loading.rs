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

//! The editor's loading screen — the door film over the whole window while
//! the editor starts.
//!
//! The panel sits in the shell's [`PanelLocation::Cover`] slot, above every
//! other panel and taking every click. It plays the film until the editor is
//! [ready](LoadingPanel::ready), plays on to the end of the chapter under way,
//! fades out, and is then [done](LoadingPanel::is_done) — the editor removes
//! it. A click or Esc only shortens the fade once the editor is ready.
//!
//! Times are seconds on the panel's own clock; [`EditorPanel::ui`] reads it
//! from the wall clock, [`LoadingPanel::paint_at`] takes it from the caller.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::editor_ui::*;
use khora_tool_ui::widgets::film::Film;
use khora_tool_ui::widgets::{LoadingScreen, LoadingState};

/// The editor's loading screen: [`Film::Door`] over the whole window until the
/// editor is ready.
pub struct LoadingPanel {
    screen: LoadingScreen,
    /// The instant the panel's clock reads 0.
    origin: Instant,
    signals: Arc<Signals>,
}

/// What the editor and the panel tell each other once the panel lives in the
/// shell: the editor is ready; the screen has faded out.
#[derive(Default)]
struct Signals {
    ready: AtomicBool,
    done: AtomicBool,
}

/// The editor's hold on a [`LoadingPanel`] it handed to the shell.
#[derive(Clone)]
pub struct LoadingHandle(Arc<Signals>);

impl LoadingHandle {
    /// The editor is ready: the panel plays on to the end of its chapter, then
    /// fades out.
    pub fn ready(&self) {
        self.0.ready.store(true, Ordering::Release);
    }

    /// Whether the panel has faded out and can be removed.
    pub fn is_done(&self) -> bool {
        self.0.done.load(Ordering::Acquire)
    }
}

impl LoadingPanel {
    /// The panel's id, the one to remove it by.
    pub const ID: &'static str = "khora.editor.loading";

    /// A loading screen starting its film at `now` on its own clock, which
    /// reads `now` at this instant. The film has its own backdrop; the theme
    /// is not needed to paint it.
    pub fn new(now: f32, _theme: UiTheme) -> Self {
        let since = Duration::try_from_secs_f32(now.max(0.0)).unwrap_or_default();
        let origin = Instant::now()
            .checked_sub(since)
            .unwrap_or_else(Instant::now);
        Self {
            screen: LoadingScreen::new(Film::Door, now, false),
            origin,
            signals: Arc::default(),
        }
    }

    /// The editor's hold on this panel once it is in the shell.
    pub fn handle(&self) -> LoadingHandle {
        LoadingHandle(Arc::clone(&self.signals))
    }

    /// The slot the panel is registered in.
    pub fn location(&self) -> PanelLocation {
        PanelLocation::Cover
    }

    /// The editor is ready: the film plays on to the end of its chapter, then
    /// fades out.
    pub fn ready(&mut self, now: f32) {
        self.screen.ready(now);
    }

    /// Whether the fade is over at `now`, and the panel can be removed.
    pub fn is_done(&self, now: f32) -> bool {
        self.screen.state(now) == LoadingState::Done
    }

    /// Paints the film over the panel rect at `now`, handling a click or Esc;
    /// returns where the screen is after painting.
    pub fn paint_at(&mut self, ui: &mut dyn UiBuilder, now: f32) -> LoadingState {
        if self.signals.ready.load(Ordering::Acquire) {
            self.ready(now);
        }
        let rect = ui.panel_rect();
        let state = self.screen.paint(ui, rect, now);
        if self.is_done(now) {
            self.signals.done.store(true, Ordering::Release);
        }
        state
    }
}

impl EditorPanel for LoadingPanel {
    fn id(&self) -> &str {
        Self::ID
    }

    fn title(&self) -> &str {
        "Loading"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let now = self.origin.elapsed().as_secs_f32();
        self.paint_at(ui, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::KeyCode;
    use khora_tool_ui::brand::khora_dark;
    use khora_tool_ui::testing::{PaintCall, RecordingUiBuilder};
    use khora_tool_ui::widgets::film::{self, Film};
    use khora_tool_ui::widgets::LoadingScreen;

    /// The window the panel covers.
    const RECT: [f32; 4] = [0.0, 0.0, 1440.0, 900.0];

    fn recorder() -> RecordingUiBuilder {
        RecordingUiBuilder::new(RECT[2], RECT[3])
    }

    fn panel_at(now: f32) -> LoadingPanel {
        LoadingPanel::new(now, khora_dark())
    }

    /// The calls a paint made, its interaction regions left out.
    fn painted(calls: &[PaintCall]) -> Vec<PaintCall> {
        calls
            .iter()
            .filter(|c| !matches!(c, PaintCall::Interact { .. }))
            .cloned()
            .collect()
    }

    /// The door film's own frame at `t` over [`RECT`].
    fn door_frame(t: f32) -> Vec<PaintCall> {
        let mut ui = recorder();
        film::paint(&mut ui, Film::Door, RECT, t);
        ui.calls().to_vec()
    }

    /// Whether `call` is a filled rect covering the whole of [`RECT`], at
    /// full opacity.
    fn is_opaque_backdrop(call: &PaintCall) -> bool {
        matches!(
            call,
            PaintCall::RectFilled { min, size, color, .. }
                if min[0] <= RECT[0] + 0.5
                    && min[1] <= RECT[1] + 0.5
                    && size[0] >= RECT[2] - 0.5
                    && size[1] >= RECT[3] - 0.5
                    && color[3] >= 0.999
        )
    }

    #[test]
    fn it_paints_the_door_film_over_its_panel_rect() {
        let mut panel = panel_at(0.0);
        let mut ui = recorder();
        assert_eq!(panel.paint_at(&mut ui, 5.0), LoadingState::Playing);
        assert_eq!(
            painted(ui.calls()),
            door_frame(5.0),
            "the door film's frame at 5 s, over the whole panel"
        );
    }

    #[test]
    fn it_paints_the_backdrop_first_over_the_whole_rect() {
        let mut panel = panel_at(0.0);
        let mut ui = recorder();
        panel.paint_at(&mut ui, 1.0);
        let calls = painted(ui.calls());
        let first = calls.first().expect("the loading screen paints");
        assert!(
            is_opaque_backdrop(first),
            "the first call is an opaque backdrop over the whole window, got {first:?}"
        );
    }

    #[test]
    fn the_film_plays_on_its_own_clock_from_the_start() {
        let mut panel = panel_at(100.0);
        let mut ui = recorder();
        panel.paint_at(&mut ui, 107.5);
        assert_eq!(painted(ui.calls()), door_frame(7.5));
    }

    #[test]
    fn a_click_before_ready_does_not_fade() {
        let mut panel = panel_at(0.0);
        let mut ui = recorder().with_click_at([720.0, 450.0]);
        assert_eq!(panel.paint_at(&mut ui, 2.0), LoadingState::Playing);

        let mut ui = recorder();
        assert_eq!(
            panel.paint_at(&mut ui, 2.0 + LoadingScreen::FADE + 0.1),
            LoadingState::Playing,
            "the click was ignored: still playing past a fade's length"
        );
        assert!(
            is_opaque_backdrop(&painted(ui.calls())[0]),
            "still at full opacity"
        );
        assert!(!panel.is_done(100.0));
    }

    #[test]
    fn esc_before_ready_does_not_fade() {
        let mut panel = panel_at(0.0);
        let mut ui = recorder().with_key(KeyCode::Escape);
        assert_eq!(panel.paint_at(&mut ui, 2.0), LoadingState::Playing);
        assert!(!panel.is_done(2.0 + LoadingScreen::FADE + 0.1));
    }

    #[test]
    fn before_ready_it_is_never_done() {
        let panel = panel_at(0.0);
        for now in [0.0, 10.0, 100.0, 1000.0] {
            assert!(!panel.is_done(now), "not done at {now} before ready");
        }
    }

    #[test]
    fn after_ready_it_finishes_the_chapter_then_is_done() {
        // Ready at film time 5.0, in "Lumière": the chapter ends at 11.3.
        let mut panel = panel_at(0.0);
        panel.ready(5.0);
        assert!(!panel.is_done(11.2), "the chapter plays on");
        assert!(!panel.is_done(11.5), "the film fades");
        assert!(
            panel.is_done(11.3 + LoadingScreen::FADE + 0.01),
            "done once the fade is over"
        );
    }

    #[test]
    fn a_click_after_ready_fades_at_once() {
        let mut panel = panel_at(0.0);
        panel.ready(5.0);
        let mut ui = recorder().with_click_at([720.0, 450.0]);
        assert_eq!(panel.paint_at(&mut ui, 5.0), LoadingState::Fading);
        assert!(panel.is_done(5.0 + LoadingScreen::FADE + 0.01));
    }

    #[test]
    fn once_done_it_paints_nothing() {
        let mut panel = panel_at(0.0);
        panel.ready(5.0);
        let mut ui = recorder();
        assert_eq!(panel.paint_at(&mut ui, 20.0), LoadingState::Done);
        assert_eq!(ui.calls(), &[], "a finished loading screen paints nothing");
    }

    #[test]
    fn it_sits_in_the_cover_slot() {
        assert_eq!(panel_at(0.0).location(), PanelLocation::Cover);
    }

    #[test]
    fn its_id_is_stable() {
        let a = panel_at(0.0);
        let b = panel_at(50.0);
        assert_eq!(a.id(), LoadingPanel::ID);
        assert_eq!(
            b.id(),
            LoadingPanel::ID,
            "the id does not depend on the clock"
        );
    }

    #[test]
    fn the_panel_ui_paints_the_film_on_the_wall_clock() {
        let mut panel = panel_at(0.0);
        let mut ui = recorder();
        panel.ui(&mut ui);
        let calls = painted(ui.calls());
        assert!(
            calls.first().is_some_and(is_opaque_backdrop),
            "the panel paints the film's backdrop over its rect, got {:?}",
            calls.first()
        );
    }
}
