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

//! The egui drag hovering a region: its payload is read there — so a slot
//! can light for what it would accept — and stays the drag's, for the drop.

use std::collections::HashMap;

use khora_core::ui::editor::UiBuilder;
use khora_infra::EguiUiBuilder;

/// A slot, `20,20 → 200×22`.
const SLOT: [f32; 4] = [20.0, 20.0, 200.0, 22.0];
/// A point inside it, and one outside.
const INSIDE: [f32; 2] = [60.0, 30.0];
const OUTSIDE: [f32; 2] = [350.0, 150.0];
/// What the drag carries: an asset-tile payload.
const PAYLOAD: u64 = 0x4B48_5046_0000_0002;

/// One egui frame with the pointer at `pointer`: what the slot read as the
/// hovering payload, twice in a row.
fn frame(ctx: &egui::Context, pointer: [f32; 2]) -> (Option<u64>, Option<u64>) {
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
    let mut read = (None, None);
    let _ = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        builder.interact_rect("slot", SLOT);
        read = (builder.dnd_hover_payload(), builder.dnd_hover_payload());
    });
    read
}

/// A few frames at `pointer`, so the slot of one frame is what the next one's
/// pointer is judged against; what the last one read.
fn settle(ctx: &egui::Context, pointer: [f32; 2]) -> (Option<u64>, Option<u64>) {
    let mut read = frame(ctx, pointer);
    for _ in 0..3 {
        read = frame(ctx, pointer);
    }
    read
}

/// **A drag over the region shows its payload, and keeps it**: read twice in
/// a frame, it is the same; after the frame, the drag still carries it.
#[test]
fn a_drag_hovering_a_region_shows_its_payload_without_giving_it_up() {
    let ctx = egui::Context::default();
    settle(&ctx, INSIDE);
    egui::DragAndDrop::set_payload(&ctx, PAYLOAD);
    let (first, second) = settle(&ctx, INSIDE);
    assert_eq!(first, Some(PAYLOAD), "the hovering drag's payload");
    assert_eq!(second, Some(PAYLOAD), "reading it did not take it");
    assert_eq!(
        egui::DragAndDrop::payload::<u64>(&ctx).map(|payload| *payload),
        Some(PAYLOAD),
        "the drag still carries it, for the drop"
    );
}

/// **Only a drag over the region shows its payload there**: none with no
/// drag, none once the drag has left the region.
#[test]
fn a_drag_shows_its_payload_only_over_the_region() {
    let ctx = egui::Context::default();
    assert_eq!(settle(&ctx, INSIDE), (None, None), "no drag at all");

    egui::DragAndDrop::set_payload(&ctx, PAYLOAD);
    assert_eq!(
        settle(&ctx, INSIDE).0,
        Some(PAYLOAD),
        "the drag over the region"
    );
    assert_eq!(
        settle(&ctx, OUTSIDE),
        (None, None),
        "the drag, moved outside the region"
    );
}
