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

//! The egui bounded drag value: every edit the author makes — a drag, a typed
//! value, an arrow-key step — stays between its bounds.

use std::collections::HashMap;

use khora_core::ui::editor::UiBuilder;
use khora_infra::EguiUiBuilder;

/// One egui frame drawing a drag value bounded to `0..=100`, fed `events`;
/// `focus` gives it keyboard focus. Returns whether it reported a change.
fn frame(ctx: &egui::Context, events: Vec<egui::Event>, value: &mut f32, focus: bool) -> bool {
    let textures = HashMap::new();
    let mut changed = false;
    let input = egui::RawInput {
        events,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 200.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        changed |= builder.drag_value_f32_in("", value, 0.5, 0.0, 100.0);
        if focus {
            builder.focus_last_item();
        }
    });
    changed
}

fn arrow_up() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::ArrowUp,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn an_arrow_key_step_at_the_maximum_stays_at_the_maximum() {
    let ctx = egui::Context::default();
    let mut value = 100.0_f32;
    frame(&ctx, Vec::new(), &mut value, true);
    frame(&ctx, Vec::new(), &mut value, false);
    let changed = frame(&ctx, vec![arrow_up()], &mut value, false);
    assert!(
        value <= 100.0,
        "an arrow-key step from the maximum left {value}, past 0–100 (changed: {changed})"
    );
}
