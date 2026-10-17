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

//! A region with a widget drawn above it: the pointer on that widget is not
//! the region's hover, but it is still inside the region — what a row shows
//! its own actions by.

use std::collections::HashMap;

use khora_core::ui::editor::{Interaction, UiBuilder};
use khora_infra::EguiUiBuilder;

/// A row, `0,0 → 300×60`, and a button drawn above it at `200,10 → 40×40`.
const ROW: [f32; 4] = [0.0, 0.0, 300.0, 60.0];
const BUTTON: [f32; 4] = [200.0, 10.0, 40.0, 40.0];

/// One egui frame with the pointer at `pointer`: what the row and the button
/// report.
fn frame(ctx: &egui::Context, pointer: [f32; 2]) -> (Interaction, Interaction) {
    let textures = HashMap::new();
    let input = egui::RawInput {
        events: vec![egui::Event::PointerMoved(egui::pos2(
            pointer[0], pointer[1],
        ))],
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 200.0),
        )),
        ..Default::default()
    };
    let mut seen = (Interaction::default(), Interaction::default());
    let _ = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        let row = builder.interact_rect("row", ROW);
        let button = builder.interact_rect("row-action", BUTTON);
        seen = (row, button);
    });
    seen
}

/// **The pointer on a row's action is still inside the row**: the action
/// takes the row's hover, and the row still says the pointer is on it.
#[test]
fn a_region_under_a_pointed_widget_still_contains_the_pointer() {
    let ctx = egui::Context::default();
    // A few frames, so the widgets of one frame are what the next one's hover
    // is judged against.
    let mut seen = frame(&ctx, [220.0, 30.0]);
    for _ in 0..3 {
        seen = frame(&ctx, [220.0, 30.0]);
    }
    let (row, button) = seen;
    assert!(button.hovered, "the button is under the pointer");
    assert!(
        !row.hovered,
        "the button, drawn above, takes the row's hover"
    );
    assert!(row.contains_pointer, "the pointer is still inside the row");
}

/// **A row pointed at elsewhere is both hovered and pointed at**; one the
/// pointer has left is neither.
#[test]
fn a_region_pointed_at_directly_is_hovered_and_one_left_is_not() {
    let ctx = egui::Context::default();
    let mut seen = frame(&ctx, [40.0, 30.0]);
    for _ in 0..3 {
        seen = frame(&ctx, [40.0, 30.0]);
    }
    assert!(seen.0.hovered && seen.0.contains_pointer);
    assert!(!seen.1.hovered && !seen.1.contains_pointer);

    for _ in 0..3 {
        seen = frame(&ctx, [350.0, 150.0]);
    }
    assert!(!seen.0.hovered && !seen.0.contains_pointer);
}
