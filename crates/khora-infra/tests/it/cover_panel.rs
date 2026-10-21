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

//! The egui shell's `Cover` slot: one panel over the whole screen, painted
//! above every other panel — floating ones included — and taking the pointer
//! from everything under it, until it is removed.

use std::sync::{Arc, Mutex};

use khora_core::ui::editor::panel::{EditorPanel, PanelLocation};
use khora_core::ui::editor::shell::EditorShell;
use khora_core::ui::editor::UiBuilder;
use khora_core::ui::UiTheme;
use khora_infra::EguiEditorShell;

/// The screen the shell is shown on.
const SCREEN: [f32; 2] = [800.0, 600.0];

/// The colour the cover paints with.
const COVER_COLOUR: [f32; 4] = [1.0, 0.0, 1.0, 1.0];
/// The colour the floating panel paints with.
const FLOATING_COLOUR: [f32; 4] = [0.0, 1.0, 1.0, 1.0];

/// Where the floating panel paints, and where its region is.
const FLOATING_RECT: [f32; 4] = [100.0, 100.0, 200.0, 150.0];

/// What a [`Probe`] saw on its last frame.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    /// Its `panel_rect`.
    panel_rect: Option<[f32; 4]>,
    /// Whether its region reported the pointer hovering it.
    hovered: bool,
}

/// A panel that paints one filled rect, over its panel rect or over a fixed
/// rect, with a region on it, and records what it saw.
struct Probe {
    id: &'static str,
    colour: [f32; 4],
    /// `None`: fill the panel rect.
    rect: Option<[f32; 4]>,
    seen: Arc<Mutex<Seen>>,
}

impl EditorPanel for Probe {
    fn id(&self) -> &str {
        self.id
    }
    fn title(&self) -> &str {
        self.id
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let panel_rect = ui.panel_rect();
        let r = self.rect.unwrap_or(panel_rect);
        ui.paint_rect_filled([r[0], r[1]], [r[2], r[3]], self.colour, 0.0);
        let hovered = self
            .rect
            .map(|rect| ui.interact_rect(self.id, rect).hovered)
            .unwrap_or(false);
        *self.seen.lock().expect("the probe lock") = Seen {
            panel_rect: Some(panel_rect),
            hovered,
        };
    }
}

/// A probe and the handle to what it sees.
fn probe(id: &'static str, colour: [f32; 4], rect: Option<[f32; 4]>) -> (Probe, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    (
        Probe {
            id,
            colour,
            rect,
            seen: Arc::clone(&seen),
        },
        seen,
    )
}

fn input(pointer: Option<[f32; 2]>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(SCREEN[0], SCREEN[1]),
        )),
        events: pointer
            .map(|p| vec![egui::Event::PointerMoved(egui::pos2(p[0], p[1]))])
            .unwrap_or_default(),
        ..Default::default()
    }
}

/// One headless frame of the shell; the shapes it painted, in paint order.
fn frame(
    ctx: &egui::Context,
    shell: &mut EguiEditorShell,
    pointer: Option<[f32; 2]>,
) -> Vec<egui::epaint::ClippedShape> {
    ctx.begin_pass(input(pointer));
    shell.show_frame();
    ctx.end_pass().shapes
}

/// Whether `fill` is `c` at some opacity. `c`'s channels are each 0 or 1, so
/// a premultiplied fill of it has every lit channel equal to its alpha — egui
/// fades a new area in, so the alpha alone is not the panel's.
fn is_colour(fill: egui::Color32, c: [f32; 4]) -> bool {
    let a = fill.a();
    let channel = |got: u8, want: f32| if want > 0.5 { got == a } else { got == 0 };
    a > 0 && channel(fill.r(), c[0]) && channel(fill.g(), c[1]) && channel(fill.b(), c[2])
}

/// The paint-order index of the first filled rect of colour `c`.
fn index_of_fill(shapes: &[egui::epaint::ClippedShape], c: [f32; 4]) -> Option<usize> {
    shapes
        .iter()
        .position(|s| matches!(&s.shape, egui::Shape::Rect(r) if is_colour(r.fill, c)))
}

fn shell() -> (egui::Context, EguiEditorShell) {
    let ctx = egui::Context::default();
    let shell = EguiEditorShell::new(ctx.clone(), UiTheme::default());
    (ctx, shell)
}

fn assert_rect_near(got: [f32; 4], want: [f32; 4], what: &str) {
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= 0.5, "{what}: got {got:?}, want {want:?}");
    }
}

#[test]
fn a_cover_panel_is_painted_above_a_floating_panel() {
    let (ctx, mut shell) = shell();
    let (cover, _) = probe("test.cover", COVER_COLOUR, None);
    let (floating, _) = probe("test.floating", FLOATING_COLOUR, Some(FLOATING_RECT));
    // The cover is registered first: its place on screen must not depend on
    // the order panels were registered in.
    shell.register_panel(PanelLocation::Cover, Box::new(cover));
    shell.register_panel(PanelLocation::Floating(100), Box::new(floating));

    // A new egui area is hidden on its first frame, while it measures itself.
    frame(&ctx, &mut shell, None);
    let shapes = frame(&ctx, &mut shell, None);

    let floating_at = index_of_fill(&shapes, FLOATING_COLOUR).expect("the floating panel painted");
    let cover_at = index_of_fill(&shapes, COVER_COLOUR).expect("the cover painted");
    assert!(
        cover_at > floating_at,
        "the cover is painted after — above — the floating panel ({cover_at} vs {floating_at})"
    );
}

#[test]
fn a_cover_panel_gets_the_whole_screen_as_its_panel_rect() {
    let (ctx, mut shell) = shell();
    let (cover, seen) = probe("test.cover", COVER_COLOUR, None);
    shell.register_panel(PanelLocation::Cover, Box::new(cover));

    for _ in 0..2 {
        frame(&ctx, &mut shell, None);
    }

    let rect = seen
        .lock()
        .expect("the probe lock")
        .panel_rect
        .expect("the cover was shown");
    assert_rect_near(
        rect,
        [0.0, 0.0, SCREEN[0], SCREEN[1]],
        "the cover's panel rect",
    );
}

#[test]
fn a_cover_panel_takes_the_pointer_from_the_panels_under_it() {
    let (ctx, mut shell) = shell();
    let (floating, seen) = probe("test.floating", FLOATING_COLOUR, Some(FLOATING_RECT));
    shell.register_panel(PanelLocation::Floating(100), Box::new(floating));
    let pointer = Some([150.0, 150.0]);

    // Without a cover the floating panel's region has the pointer — a few
    // frames, so each frame's hover is judged against the last one's widgets.
    for _ in 0..3 {
        frame(&ctx, &mut shell, pointer);
    }
    assert!(
        seen.lock().expect("the probe lock").hovered,
        "uncovered, the floating panel's region is hovered"
    );

    let (cover, _) = probe("test.cover", COVER_COLOUR, None);
    shell.register_panel(PanelLocation::Cover, Box::new(cover));
    for _ in 0..3 {
        frame(&ctx, &mut shell, pointer);
    }
    assert!(
        !seen.lock().expect("the probe lock").hovered,
        "under the cover, the floating panel's region is not hovered"
    );
}

#[test]
fn a_removed_cover_panel_is_no_longer_painted() {
    let (ctx, mut shell) = shell();
    let (cover, _) = probe("test.cover", COVER_COLOUR, None);
    shell.register_panel(PanelLocation::Cover, Box::new(cover));

    assert!(
        shell.remove_panel("test.cover"),
        "the cover is found and removed"
    );
    frame(&ctx, &mut shell, None);
    let shapes = frame(&ctx, &mut shell, None);
    assert_eq!(
        index_of_fill(&shapes, COVER_COLOUR),
        None,
        "a removed cover paints nothing"
    );
    assert!(
        !shell.remove_panel("test.cover"),
        "the cover is gone: a second removal finds nothing"
    );
}

/// A cover panel that asks for clicks over its whole rect, and counts them.
struct ClickCounter {
    clicks: Arc<Mutex<usize>>,
}

impl EditorPanel for ClickCounter {
    fn id(&self) -> &str {
        "test.click_cover"
    }
    fn title(&self) -> &str {
        "test.click_cover"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let rect = ui.panel_rect();
        ui.paint_rect_filled([rect[0], rect[1]], [rect[2], rect[3]], COVER_COLOUR, 0.0);
        if ui.interact_rect("test.click_cover.region", rect).clicked {
            *self.clicks.lock().expect("the click lock") += 1;
        }
    }
}

/// One headless frame of the shell receiving `events`.
fn pointer_frame(ctx: &egui::Context, shell: &mut EguiEditorShell, events: Vec<egui::Event>) {
    let mut raw = input(None);
    raw.events = events;
    ctx.begin_pass(raw);
    shell.show_frame();
    let _ = ctx.end_pass();
}

/// The cover takes the clicks for itself — the shell's own hold on the
/// screen must not keep them from the panel it shows, or a loading screen
/// could never be clicked away.
#[test]
fn a_click_on_a_cover_panel_reaches_the_cover_panel() {
    let (ctx, mut shell) = shell();
    let clicks = Arc::new(Mutex::new(0));
    shell.register_panel(
        PanelLocation::Cover,
        Box::new(ClickCounter {
            clicks: Arc::clone(&clicks),
        }),
    );
    let at = egui::pos2(400.0, 300.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    for _ in 0..2 {
        pointer_frame(&ctx, &mut shell, vec![egui::Event::PointerMoved(at)]);
    }
    pointer_frame(&ctx, &mut shell, vec![button(true)]);
    pointer_frame(&ctx, &mut shell, vec![button(false)]);
    pointer_frame(&ctx, &mut shell, vec![]);

    assert_eq!(
        *clicks.lock().expect("the click lock"),
        1,
        "the cover saw the click"
    );
}
