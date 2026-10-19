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

//! The egui menu a primary click opens on a region — an asset slot's picker:
//! it opens on that click, stays open, and closes on a click outside.

use std::collections::HashMap;

use khora_core::ui::editor::UiBuilder;
use khora_infra::EguiUiBuilder;

/// A slot, `20,20 → 200×22`.
const SLOT: [f32; 4] = [20.0, 20.0, 200.0, 22.0];
/// A point on it, and one far from it and from any menu it opens.
const ON: [f32; 2] = [60.0, 30.0];
const AWAY: [f32; 2] = [380.0, 380.0];

/// One egui frame fed `events`: whether the slot's menu drew its content.
fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> bool {
    let textures = HashMap::new();
    let input = egui::RawInput {
        events,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 400.0),
        )),
        ..Default::default()
    };
    let mut shown = false;
    let _ = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        builder.interact_rect("slot", SLOT);
        builder.menu_on_click_last(&mut |menu| {
            shown = true;
            menu.label("bullet");
        });
    });
    shown
}

fn moved(at: [f32; 2]) -> egui::Event {
    egui::Event::PointerMoved(egui::pos2(at[0], at[1]))
}

fn button(at: [f32; 2], button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui::pos2(at[0], at[1]),
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// A click of `which` at `at`, over three frames — there, down, up. Whether
/// the menu drew its content in any of them.
fn click(ctx: &egui::Context, at: [f32; 2], which: egui::PointerButton) -> bool {
    let mut shown = frame(ctx, vec![moved(at)]);
    shown |= frame(ctx, vec![button(at, which, true)]);
    shown |= frame(ctx, vec![button(at, which, false)]);
    shown
}

/// Whether the menu draws its content in any of a few quiet frames.
fn shown_after(ctx: &egui::Context) -> bool {
    (0..3).fold(false, |shown, _| frame(ctx, Vec::new()) | shown)
}

/// **A primary click on the region opens its menu, which stays open; a
/// click outside closes it.**
#[test]
fn a_primary_click_on_a_region_opens_its_menu_until_a_click_outside() {
    let ctx = egui::Context::default();
    for _ in 0..3 {
        assert!(!frame(&ctx, vec![moved(ON)]), "closed before any click");
    }

    let opened = click(&ctx, ON, egui::PointerButton::Primary) | shown_after(&ctx);
    assert!(opened, "a primary click on the slot opens its menu");
    assert!(
        frame(&ctx, Vec::new()),
        "the menu stays open while nothing is picked"
    );

    click(&ctx, AWAY, egui::PointerButton::Primary);
    let mut still = false;
    for _ in 0..2 {
        still = frame(&ctx, vec![moved(AWAY)]);
    }
    assert!(!still, "a click outside closes the menu");
}

/// **A secondary click is the context menu's, not this one's.**
#[test]
fn a_secondary_click_does_not_open_a_click_menu() {
    let ctx = egui::Context::default();
    let opened = click(&ctx, ON, egui::PointerButton::Secondary) | shown_after(&ctx);
    assert!(!opened, "a secondary click opened the click menu");

    let opened = click(&ctx, ON, egui::PointerButton::Primary) | shown_after(&ctx);
    assert!(
        opened,
        "a primary click opens it — the contrast this test needs"
    );
}

/// What the menu of [`searching`] showed in a frame.
#[derive(Default)]
struct Searched {
    /// The menu drew its content.
    shown: bool,
    /// Where its search field was placed.
    field_at: Option<[f32; 2]>,
    /// The pointer was on the search field.
    on_field: bool,
}

/// One egui frame fed `events` of a slot whose menu holds a search field
/// editing `query`.
fn searching(ctx: &egui::Context, events: Vec<egui::Event>, query: &mut String) -> Searched {
    let textures = HashMap::new();
    let input = egui::RawInput {
        events,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 400.0),
        )),
        ..Default::default()
    };
    let mut seen = Searched::default();
    let _ = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        builder.interact_rect("slot", SLOT);
        builder.menu_on_click_last(&mut |menu| {
            seen.shown = true;
            seen.field_at = Some(menu.cursor_pos());
            menu.text_edit_singleline(query);
            seen.on_field = menu.is_last_item_hovered();
            menu.label("bullet");
        });
    });
    seen
}

/// **A click in the menu that picks nothing — into its search field — keeps
/// it open, and what is typed there reaches the field.** The menu closes on
/// a pick or a click outside, nothing else.
#[test]
fn a_click_into_the_menus_search_field_keeps_it_open_for_typing() {
    let ctx = egui::Context::default();
    let mut query = String::new();
    searching(&ctx, vec![moved(ON)], &mut query);
    searching(
        &ctx,
        vec![button(ON, egui::PointerButton::Primary, true)],
        &mut query,
    );
    searching(
        &ctx,
        vec![button(ON, egui::PointerButton::Primary, false)],
        &mut query,
    );
    let open = searching(&ctx, Vec::new(), &mut query);
    assert!(open.shown, "a primary click on the slot opens its menu");
    let at = open.field_at.expect("the menu placed its search field");
    let field = [at[0] + 20.0, at[1] + 8.0];

    let hovered = searching(&ctx, vec![moved(field)], &mut query);
    assert!(hovered.on_field, "the pointer is on the search field");
    searching(
        &ctx,
        vec![button(field, egui::PointerButton::Primary, true)],
        &mut query,
    );
    searching(
        &ctx,
        vec![button(field, egui::PointerButton::Primary, false)],
        &mut query,
    );
    let after = searching(&ctx, Vec::new(), &mut query);
    assert!(after.shown, "a click into the search field closed the menu");

    searching(&ctx, vec![egui::Event::Text("rock".to_owned())], &mut query);
    assert_eq!(query, "rock", "what is typed reaches the search field");
}

/// What the menu of [`picking`] showed and did in a frame.
#[derive(Default)]
struct Picker {
    /// The menu drew its content.
    shown: bool,
    /// Where its one entry was placed.
    entry_at: Option<[f32; 2]>,
    /// The pointer was on that entry.
    on_entry: bool,
    /// The entry was clicked: picked, and the menu asked to close.
    picked: bool,
}

/// One egui frame fed `events` of a slot whose menu is a picker — a search
/// field, then one entry, `rocket`, whose click picks it and closes the
/// menu, as an asset slot's picker does.
fn picking(ctx: &egui::Context, events: Vec<egui::Event>) -> Picker {
    let textures = HashMap::new();
    let input = egui::RawInput {
        events,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(400.0, 400.0),
        )),
        ..Default::default()
    };
    let mut seen = Picker::default();
    let mut query = String::new();
    let _ = ctx.run_ui(input, |ui| {
        let mut builder = EguiUiBuilder::new(ui, &textures);
        builder.interact_rect("slot", SLOT);
        builder.menu_on_click_last(&mut |menu| {
            seen.shown = true;
            menu.text_edit_singleline(&mut query);
            seen.entry_at = Some(menu.cursor_pos());
            if menu.selectable_label(false, "rocket") {
                seen.picked = true;
                menu.close_menu();
            }
            seen.on_entry |= menu.is_last_item_hovered();
        });
    });
    seen
}

/// A primary click at `at` over three frames; what the last one showed.
fn picking_click(ctx: &egui::Context, at: [f32; 2]) -> Picker {
    let mut seen = picking(ctx, vec![moved(at)]);
    let down = picking(ctx, vec![button(at, egui::PointerButton::Primary, true)]);
    let up = picking(ctx, vec![button(at, egui::PointerButton::Primary, false)]);
    seen.picked |= down.picked | up.picked;
    seen.on_entry |= down.on_entry;
    seen.shown = up.shown;
    seen
}

/// Whether the picker draws its content in any of a few quiet frames.
fn picker_shown_after(ctx: &egui::Context) -> bool {
    (0..3).fold(false, |shown, _| picking(ctx, Vec::new()).shown | shown)
}

/// **A pick closes the menu**: it stays open for a click into it that picks
/// nothing, so the pick itself must close it.
#[test]
fn a_pick_in_a_click_menu_closes_it() {
    let ctx = egui::Context::default();
    picking_click(&ctx, ON);
    let open = picking(&ctx, Vec::new());
    assert!(open.shown, "a primary click on the slot opens its menu");
    let at = open.entry_at.expect("the menu placed its entry");
    let entry = [at[0] + 20.0, at[1] + 8.0];

    let clicked = picking_click(&ctx, entry);
    assert!(clicked.on_entry, "the pointer is on the entry");
    assert!(clicked.picked, "the click on the entry picks it");
    assert!(!picker_shown_after(&ctx), "the pick left the menu open");
}

/// **A second click on the region closes the menu its first click opened**
/// — the region is outside the menu.
#[test]
fn a_second_click_on_the_region_closes_its_open_menu() {
    let ctx = egui::Context::default();
    picking_click(&ctx, ON);
    assert!(
        picker_shown_after(&ctx),
        "a primary click on the slot opens its menu"
    );

    picking_click(&ctx, ON);
    assert!(
        !picker_shown_after(&ctx),
        "a second click on the slot left its menu open"
    );
}
