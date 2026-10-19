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

//! An asset slot — a field typed by a kind of asset, `Prefab? bullet;`: what
//! it says, what a drag over it shows, and what a drop, a pick or its menu
//! writes.

use std::collections::HashMap;

use khora_sdk::editor_ui::{
    AssetEntry, DeclaredCard, DeclaredCardKind, DeclaredFieldView, EditorState, Icon,
    InspectedEntity, PropertyEdit, UiTheme,
};
use khora_sdk::khora_core::asset::{AssetKind, AssetUUID};
use khora_sdk::khora_core::script::{ErgonType, FieldHints, ScriptValue};
use khora_sdk::khora_data::ecs::SemanticDomain;
use khora_sdk::prelude::ecs::EntityId;
use khora_tool_ui::widgets::DOCK_DRAG_TAG;

use super::super::asset_slot::{asset_text, picker_entries};
use super::super::fields::{render_field, value_text, FieldContext};
use super::super::render_declared_cards;
use super::rendering::{Drawn, Input, Spy};
use crate::drag_payload::{pack_asset_drag, pack_entity};

/// The epoch the project's asset list was read at.
const EPOCH: u64 = 7;

/// Where each asset of [`project`] sits in it.
const BULLET: usize = 0;
const CRATE: usize = 1;
const ROCKET: usize = 2;

/// An entry of the asset browser for the file at `path`, of the index's
/// `asset_type`.
fn entry(path: &str, asset_type: &str) -> AssetEntry {
    AssetEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        asset_type: asset_type.to_owned(),
        source_path: path.to_owned(),
        uuid: AssetUUID::new_v5(path),
    }
}

/// A project's assets, in the order the browser lists them: three prefabs,
/// a mesh, a sound and a script.
fn project() -> Vec<AssetEntry> {
    vec![
        entry("prefabs/bullet.kprefab", "prefab"),
        entry("meshes/crate.obj", "mesh"),
        entry("prefabs/weapons/rocket.kprefab", "prefab"),
        entry("sounds/alarm.wav", "audio"),
        entry("anvil.kprefab", "prefab"),
        entry("scripts/guard.erg", "script"),
    ]
}

/// The identity of the asset at `at` in [`project`].
fn uuid(at: usize) -> AssetUUID {
    project()[at].uuid
}

/// An identity no entry of [`project`] holds; its first group is `abcdef01`.
fn gone() -> AssetUUID {
    AssetUUID::from_bytes([
        0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0, 0, 0, 0, 0, 0, 0, 0x42,
    ])
}

/// A drag of the asset at `at` in [`project`], started at [`EPOCH`].
fn dragged(at: usize) -> u64 {
    pack_asset_drag(at as u32, EPOCH)
}

fn asset(kind: AssetKind) -> ErgonType {
    ErgonType::Asset(kind)
}

fn optional(ty: ErgonType) -> ErgonType {
    ErgonType::Optional(Box::new(ty))
}

/// Draws, editable, the field `bullet` of type `ty` holding `value` in a
/// project holding [`project`], the pointer bringing `input`. Returns the
/// builder and whether the field reported a change.
fn draw(ty: &ErgonType, value: &mut ScriptValue, input: Input) -> (Spy, bool) {
    let theme = UiTheme::default();
    let names = HashMap::new();
    let assets = project();
    let cx = FieldContext {
        theme: &theme,
        names: &names,
        id: "declared.1.Armory.bullet".to_owned(),
        range: None,
        tooltip: None,
        assets: &assets,
        asset_epoch: EPOCH,
        memory: None,
    };
    let mut ui = Spy::with(input);
    let changed = render_field(&mut ui, "bullet", ty, value, false, &cx);
    (ui, changed)
}

/// The author clicks every button whose text holds one of `texts`.
fn clicking(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|text| (*text).to_owned()).collect()
}

/// The field is drawn as an asset slot: its kind's icon is painted.
fn assert_slot(ui: &Spy, icon: Icon) {
    assert!(
        ui.glyphs().iter().any(|(glyph, _)| glyph == icon.glyph()),
        "the slot paints no {icon:?} icon: {:?}",
        ui.drawn()
    );
}

// ─── What an asset reads as ─────────────────────────────────────────────────

#[test]
fn an_asset_reads_as_the_stem_of_the_entry_holding_it() {
    let assets = project();
    assert_eq!(asset_text(uuid(BULLET), &assets), "bullet");
    assert_eq!(
        asset_text(uuid(ROCKET), &assets),
        "rocket",
        "the folder is not part of its name"
    );
}

#[test]
fn an_asset_no_entry_holds_reads_missing_and_its_first_eight_hex_digits() {
    assert_eq!(asset_text(gone(), &project()), "missing · abcdef01");
    assert_eq!(
        asset_text(uuid(BULLET), &[]),
        format!("missing · {}", &uuid(BULLET).to_string()[..8]),
        "a project holding no asset holds none of them"
    );
}

/// The read-only row says what the slot says.
#[test]
fn a_read_asset_value_names_its_asset_or_says_it_is_missing() {
    let names = HashMap::new();
    let assets = project();
    let prefab = asset(AssetKind::Prefab);
    assert_eq!(
        value_text(&ScriptValue::Asset(uuid(BULLET)), &prefab, &names, &assets),
        "bullet"
    );
    assert_eq!(
        value_text(
            &ScriptValue::Asset(uuid(ROCKET)),
            &optional(prefab.clone()),
            &names,
            &assets
        ),
        "rocket"
    );
    assert_eq!(
        value_text(&ScriptValue::Asset(gone()), &prefab, &names, &assets),
        "missing · abcdef01"
    );
    assert_eq!(
        value_text(&ScriptValue::Null, &optional(prefab), &names, &assets),
        "none"
    );
}

/// A read-only `Prefab?` row — a system's, a state's — names the asset too.
#[test]
fn a_read_only_asset_row_names_its_asset() {
    let theme = UiTheme::default();
    let names = HashMap::new();
    let assets = project();
    let cx = FieldContext {
        theme: &theme,
        names: &names,
        id: "declared.1.Armory.bullet".to_owned(),
        range: None,
        tooltip: None,
        assets: &assets,
        asset_epoch: EPOCH,
        memory: None,
    };
    let mut ui = Spy::default();
    let mut value = ScriptValue::Asset(uuid(BULLET));
    render_field(
        &mut ui,
        "bullet",
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        true,
        &cx,
    );
    assert!(
        ui.texts().iter().any(|(text, _)| text == "bullet"),
        "{:?}",
        ui.texts()
    );
}

// ─── What a picker offers ───────────────────────────────────────────────────

#[test]
fn a_picker_offers_the_entries_of_its_kind_sorted_by_name() {
    let assets = project();
    let names = |kind| -> Vec<String> {
        picker_entries(kind, &assets)
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    };
    assert_eq!(
        names(AssetKind::Prefab),
        ["anvil.kprefab", "bullet.kprefab", "rocket.kprefab"]
    );
    assert_eq!(
        names(AssetKind::Sound),
        ["alarm.wav"],
        "a sound is the index's `audio`"
    );
    assert_eq!(names(AssetKind::Mesh), ["crate.obj"]);
    assert!(names(AssetKind::Scene).is_empty());
}

#[test]
fn an_entry_of_no_asset_kind_is_offered_by_no_picker() {
    let assets = project();
    for kind in AssetKind::ALL {
        let offered = picker_entries(kind, &assets);
        assert!(
            offered.iter().all(|entry| entry.asset_type != "script"),
            "{kind:?} offers a script: {offered:?}"
        );
    }
}

// ─── What the slot says ─────────────────────────────────────────────────────

#[test]
fn a_held_asset_slot_shows_its_kinds_icon_its_name_and_its_folder() {
    let theme = UiTheme::default();
    let mut value = ScriptValue::Asset(uuid(BULLET));
    let (ui, changed) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        Input::default(),
    );
    assert!(!changed);
    assert_slot(&ui, Icon::Prefab);
    let texts = ui.texts();
    assert!(
        texts.iter().any(|(text, _)| text.contains("bullet")),
        "the slot names the asset: {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|(text, color)| text.contains("prefabs") && *color == theme.text_muted),
        "the slot shows its folder, muted: {texts:?}"
    );
    assert!(
        ui.said_tooltips()
            .iter()
            .any(|tip| tip.contains("prefabs/bullet.kprefab")),
        "the full path is the tooltip: {:?}",
        ui.said_tooltips()
    );
}

#[test]
fn an_empty_asset_slot_asks_for_an_asset_of_its_kind() {
    let theme = UiTheme::default();
    let mut value = ScriptValue::Null;
    let (ui, _) = draw(
        &optional(asset(AssetKind::Sound)),
        &mut value,
        Input::default(),
    );
    assert_slot(&ui, Icon::Music);
    assert!(
        ui.texts()
            .iter()
            .any(|(text, color)| text == "none — drop a sound" && *color == theme.text_muted),
        "{:?}",
        ui.texts()
    );
}

#[test]
fn a_slot_holding_an_asset_gone_from_the_project_says_so_in_amber() {
    let theme = UiTheme::default();
    let mut value = ScriptValue::Asset(gone());
    let (ui, changed) = draw(&asset(AssetKind::Prefab), &mut value, Input::default());
    assert!(!changed);
    assert_eq!(value, ScriptValue::Asset(gone()), "showing it keeps it");
    assert!(
        ui.texts()
            .iter()
            .any(|(text, color)| text.contains("missing · abcdef01") && *color == theme.warning),
        "{:?}",
        ui.texts()
    );
    assert!(
        ui.glyphs()
            .iter()
            .any(|(glyph, _)| glyph == Icon::Warn.glyph()),
        "a warning icon: {:?}",
        ui.glyphs()
    );
    let said = ui.said_tooltips().join("\n");
    assert!(
        said.contains("This prefab is no longer in the project."),
        "{said}"
    );
    assert!(said.contains("Pick another, or set to none."), "{said}");
}

// ─── A drag over it, a drop on it ───────────────────────────────────────────

/// A drag of `payload` over the slot, released there.
fn dropping(payload: u64) -> Input {
    Input {
        dragging: true,
        hover: Some(payload),
        drop: Some(payload),
        pointer_on: true,
        ..Input::default()
    }
}

/// A drag of `payload`, held over the slot.
fn hovering(payload: u64) -> Input {
    Input {
        dragging: true,
        hover: Some(payload),
        pointer_on: true,
        ..Input::default()
    }
}

/// The pointer on the slot, dragging nothing.
fn pointing() -> Input {
    Input {
        pointer_on: true,
        ..Input::default()
    }
}

#[test]
fn dropping_an_asset_of_the_slots_kind_writes_its_identity() {
    for (ty, held) in [
        (optional(asset(AssetKind::Prefab)), ScriptValue::Null),
        (asset(AssetKind::Prefab), ScriptValue::Asset(uuid(BULLET))),
    ] {
        let mut value = held.clone();
        let (_, changed) = draw(&ty, &mut value, dropping(dragged(ROCKET)));
        assert_eq!(
            value,
            ScriptValue::Asset(uuid(ROCKET)),
            "{ty:?} holding {held:?}"
        );
        assert!(changed, "{ty:?}: the drop is a change");
    }
}

#[test]
fn dropping_an_asset_of_another_kind_is_taken_and_changes_nothing() {
    let mut value = ScriptValue::Asset(uuid(BULLET));
    let (ui, changed) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        dropping(dragged(CRATE)),
    );
    assert_eq!(value, ScriptValue::Asset(uuid(BULLET)));
    assert!(!changed);
    assert_eq!(ui.drop_left(), None, "the slot takes the drop");
}

#[test]
fn dropping_what_is_no_asset_of_this_list_changes_nothing() {
    for payload in [
        pack_entity(EntityId {
            index: 3,
            generation: 1,
        }),
        DOCK_DRAG_TAG,
        pack_asset_drag(ROCKET as u32, EPOCH + 1),
    ] {
        let mut value = ScriptValue::Asset(uuid(BULLET));
        let (ui, changed) = draw(
            &optional(asset(AssetKind::Prefab)),
            &mut value,
            dropping(payload),
        );
        assert_slot(&ui, Icon::Prefab);
        assert_eq!(value, ScriptValue::Asset(uuid(BULLET)), "{payload:#x}");
        assert!(!changed, "{payload:#x}");
    }
}

#[test]
fn a_drag_of_the_slots_kind_over_it_lights_it() {
    let ty = optional(asset(AssetKind::Prefab));
    let (idle, _) = draw(&ty, &mut ScriptValue::Null, pointing());
    let (lit, _) = draw(&ty, &mut ScriptValue::Null, hovering(dragged(ROCKET)));
    assert!(
        lit.strokes() > idle.strokes(),
        "the slot is outlined: idle {:?}, lit {:?}",
        idle.drawn(),
        lit.drawn()
    );
    let idle_fills = idle.fills();
    assert!(
        lit.fills().iter().any(|fill| !idle_fills.contains(fill)),
        "the slot is tinted: idle {idle_fills:?}, lit {:?}",
        lit.fills()
    );
}

/// A drag the slot would refuse draws it exactly as no drag does — the entity
/// slot's rule: no light, no message.
#[test]
fn a_drag_of_anything_else_over_an_asset_slot_leaves_it_as_it_was() {
    let ty = optional(asset(AssetKind::Prefab));
    let (idle, _) = draw(&ty, &mut ScriptValue::Null, pointing());
    let (lit, _) = draw(&ty, &mut ScriptValue::Null, hovering(dragged(ROCKET)));
    assert_ne!(
        lit.drawn(),
        idle.drawn(),
        "a prefab dragged over a prefab slot lights it — the contrast this test needs"
    );
    for (what, payload) in [
        ("a mesh", dragged(CRATE)),
        (
            "an entity",
            pack_entity(EntityId {
                index: 3,
                generation: 1,
            }),
        ),
        ("a dock tab", DOCK_DRAG_TAG),
        (
            "a prefab of an older asset list",
            pack_asset_drag(ROCKET as u32, EPOCH + 1),
        ),
    ] {
        let (ui, _) = draw(&ty, &mut ScriptValue::Null, hovering(payload));
        assert_eq!(ui.drawn(), idle.drawn(), "{what} dragged over the slot");
    }
}

// ─── Its menus ──────────────────────────────────────────────────────────────

#[test]
fn set_to_none_clears_a_held_optional_asset() {
    let mut value = ScriptValue::Asset(uuid(BULLET));
    let (ui, changed) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        Input {
            context_menu_open: true,
            clicks: clicking(&["Set to none"]),
            ..Input::default()
        },
    );
    assert_slot(&ui, Icon::Prefab);
    assert_eq!(value, ScriptValue::Null);
    assert!(changed);
}

#[test]
fn set_to_none_is_offered_only_for_a_held_optional_asset() {
    let menu = || Input {
        context_menu_open: true,
        ..Input::default()
    };
    let (held, _) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut ScriptValue::Asset(uuid(BULLET)),
        menu(),
    );
    assert_slot(&held, Icon::Prefab);
    assert!(
        held.buttons().iter().any(|text| text == "Set to none"),
        "{:?}",
        held.buttons()
    );
    for (ty, value) in [
        (asset(AssetKind::Prefab), ScriptValue::Asset(uuid(BULLET))),
        (optional(asset(AssetKind::Prefab)), ScriptValue::Null),
    ] {
        let mut value = value;
        let (ui, _) = draw(&ty, &mut value, menu());
        assert_slot(&ui, Icon::Prefab);
        assert!(
            !ui.buttons().iter().any(|text| text.contains("Set to none")),
            "{ty:?} holding {value:?}: {:?}",
            ui.buttons()
        );
    }
}

#[test]
fn the_context_menu_reveals_only_an_asset_still_in_the_project() {
    let menu = || Input {
        context_menu_open: true,
        ..Input::default()
    };
    let ty = optional(asset(AssetKind::Prefab));
    let (held, _) = draw(&ty, &mut ScriptValue::Asset(uuid(BULLET)), menu());
    assert!(
        held.buttons().iter().any(|text| text == "Reveal in Assets"),
        "{:?}",
        held.buttons()
    );
    for value in [ScriptValue::Asset(gone()), ScriptValue::Null] {
        let mut shown = value.clone();
        let (ui, _) = draw(&ty, &mut shown, menu());
        assert!(
            !ui.buttons().iter().any(|text| text == "Reveal in Assets"),
            "holding {value:?}: {:?}",
            ui.buttons()
        );
    }
}

#[test]
fn the_context_menu_offers_to_choose_another_asset() {
    let ty = optional(asset(AssetKind::Prefab));
    for value in [ScriptValue::Asset(uuid(BULLET)), ScriptValue::Null] {
        let mut shown = value.clone();
        let (ui, _) = draw(
            &ty,
            &mut shown,
            Input {
                context_menu_open: true,
                ..Input::default()
            },
        );
        assert!(
            ui.buttons().iter().any(|text| text.starts_with("Choose")),
            "holding {value:?}: {:?}",
            ui.buttons()
        );
    }
}

/// An empty `Prefab?` is the slot's own `none`: no `Set` button to give it a
/// value that would still be none.
#[test]
fn an_empty_optional_asset_draws_no_set_button_and_changes_nothing_on_its_own() {
    let mut value = ScriptValue::Null;
    let (ui, changed) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        Input {
            clicks: clicking(&["Set"]),
            ..Input::default()
        },
    );
    assert!(
        !ui.buttons().iter().any(|text| text == "Set"),
        "{:?}",
        ui.buttons()
    );
    assert_eq!(value, ScriptValue::Null);
    assert!(!changed);
}

// ─── The picker ─────────────────────────────────────────────────────────────

#[test]
fn picking_an_asset_writes_its_identity() {
    for (ty, held) in [
        (asset(AssetKind::Prefab), ScriptValue::Asset(uuid(BULLET))),
        (optional(asset(AssetKind::Prefab)), ScriptValue::Null),
    ] {
        let mut value = held.clone();
        let (_, changed) = draw(
            &ty,
            &mut value,
            Input {
                click_menu_open: true,
                clicks: clicking(&["rocket"]),
                ..Input::default()
            },
        );
        assert_eq!(
            value,
            ScriptValue::Asset(uuid(ROCKET)),
            "{ty:?} holding {held:?}"
        );
        assert!(changed, "{ty:?}: the pick is a change");
    }
}

#[test]
fn the_picker_offers_the_slots_kind_only_sorted_by_name() {
    let (ui, changed) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut ScriptValue::Null,
        Input {
            click_menu_open: true,
            ..Input::default()
        },
    );
    assert!(!changed, "an open picker is no change");
    let buttons = ui.buttons();
    let at = |stem: &str| buttons.iter().position(|text| text.contains(stem));
    let (anvil, bullet, rocket) = (at("anvil"), at("bullet"), at("rocket"));
    assert!(
        anvil.is_some() && bullet.is_some() && rocket.is_some(),
        "every prefab is offered: {buttons:?}"
    );
    assert!(anvil < bullet && bullet < rocket, "by name: {buttons:?}");
    for other in ["crate", "alarm", "guard"] {
        assert_eq!(at(other), None, "{other} is no prefab: {buttons:?}");
    }
    assert!(!ui.has_text_field(), "three entries need no search field");
}

#[test]
fn a_picker_of_more_than_eight_entries_has_a_search_field() {
    let theme = UiTheme::default();
    let names = HashMap::new();
    let assets: Vec<AssetEntry> = (0..9)
        .map(|i| entry(&format!("prefabs/p{i}.kprefab"), "prefab"))
        .collect();
    let cx = FieldContext {
        theme: &theme,
        names: &names,
        id: "declared.1.Armory.bullet".to_owned(),
        range: None,
        tooltip: None,
        assets: &assets,
        asset_epoch: EPOCH,
        memory: None,
    };
    let mut ui = Spy::with(Input {
        click_menu_open: true,
        ..Input::default()
    });
    let mut value = ScriptValue::Null;
    render_field(
        &mut ui,
        "bullet",
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        false,
        &cx,
    );
    assert!(ui.has_text_field(), "{:?}", ui.drawn());
}

// ─── A list of assets ───────────────────────────────────────────────────────

#[test]
fn adding_an_item_to_an_asset_list_pushes_only_a_picked_asset() {
    let list = ErgonType::Array(Box::new(asset(AssetKind::Prefab)));

    // The add button opens the picker: the click itself adds nothing.
    let mut value = ScriptValue::Array(Vec::new());
    let (_, changed) = draw(
        &list,
        &mut value,
        Input {
            clicks: clicking(&["+ Add item"]),
            ..Input::default()
        },
    );
    assert_eq!(
        value,
        ScriptValue::Array(Vec::new()),
        "the click adds nothing"
    );
    assert!(!changed);

    // The picker open, dismissed without a pick.
    let mut value = ScriptValue::Array(Vec::new());
    let (_, changed) = draw(
        &list,
        &mut value,
        Input {
            click_menu_open: true,
            ..Input::default()
        },
    );
    assert_eq!(
        value,
        ScriptValue::Array(Vec::new()),
        "nothing picked, nothing added"
    );
    assert!(!changed);

    // A pick.
    let mut value = ScriptValue::Array(Vec::new());
    let (_, changed) = draw(
        &list,
        &mut value,
        Input {
            click_menu_open: true,
            clicks: clicking(&["rocket"]),
            ..Input::default()
        },
    );
    assert_eq!(
        value,
        ScriptValue::Array(vec![ScriptValue::Asset(uuid(ROCKET))])
    );
    assert!(changed);
}

// ─── On a card ──────────────────────────────────────────────────────────────

/// A card's slot sees the project's assets as the browser lists them, at
/// the epoch it read them: a drop there is an edit of the field.
#[test]
fn dropping_an_asset_on_a_cards_slot_edits_its_field() {
    let entity = EntityId {
        index: 1,
        generation: 1,
    };
    let inspected = InspectedEntity {
        entity,
        name: "Armory".to_owned(),
        components_json: Vec::new(),
        prefab: None,
        declared: vec![DeclaredCard {
            name: "Armory".to_owned(),
            domain: SemanticDomain::Render,
            module: Some("armory.erg".to_owned()),
            kind: DeclaredCardKind::Component,
            read_only: false,
            orphaned: false,
            fields: vec![DeclaredFieldView {
                name: "bullet".to_owned(),
                ty: optional(asset(AssetKind::Prefab)),
                value: ScriptValue::Null,
                default: ScriptValue::Null,
                hints: FieldHints::default(),
            }],
        }],
    };
    let mut state = EditorState {
        asset_entries: project(),
        asset_epoch: EPOCH,
        ..EditorState::default()
    };
    let mut ui = Spy::with(dropping(dragged(ROCKET)));
    let mut edits = Vec::new();
    render_declared_cards(
        &mut ui,
        &inspected,
        &HashMap::new(),
        0.0,
        300.0,
        &UiTheme::default(),
        &mut state,
        &mut edits,
    );
    let said: Vec<String> = edits.iter().map(|edit| format!("{edit:?}")).collect();
    let expected = format!(
        "{:?}",
        PropertyEdit::SetDeclaredField {
            entity,
            component: "Armory".to_owned(),
            field: "bullet".to_owned(),
            value: ScriptValue::Asset(uuid(ROCKET)),
        }
    );
    assert_eq!(said, [expected]);
}

// ─── What a card's slots leave for the next frame ───────────────────────────

/// An `Armory` card holding the fields `fields`, on entity 1.
fn armory(fields: Vec<(&str, ErgonType, ScriptValue)>) -> InspectedEntity {
    InspectedEntity {
        entity: EntityId {
            index: 1,
            generation: 1,
        },
        name: "Armory".to_owned(),
        components_json: Vec::new(),
        prefab: None,
        declared: vec![DeclaredCard {
            name: "Armory".to_owned(),
            domain: SemanticDomain::Render,
            module: Some("armory.erg".to_owned()),
            kind: DeclaredCardKind::Component,
            read_only: false,
            orphaned: false,
            fields: fields
                .into_iter()
                .map(|(name, ty, value)| DeclaredFieldView {
                    name: name.to_owned(),
                    ty,
                    default: value.clone(),
                    value,
                    hints: FieldHints::default(),
                })
                .collect(),
        }],
    }
}

/// One frame of `inspected`'s cards in `state`, the pointer bringing
/// `input`: the builder, and the edits gathered.
fn frame(inspected: &InspectedEntity, state: &mut EditorState, input: Input) -> (Spy, Vec<String>) {
    let mut ui = Spy::with(input);
    let mut edits = Vec::new();
    render_declared_cards(
        &mut ui,
        inspected,
        &HashMap::new(),
        0.0,
        300.0,
        &UiTheme::default(),
        state,
        &mut edits,
    );
    let said = edits.iter().map(|edit| format!("{edit:?}")).collect();
    (ui, said)
}

/// A project of nine prefabs, named so that no name holds another, and an
/// alarm sound.
fn arsenal() -> Vec<AssetEntry> {
    let mut assets: Vec<AssetEntry> = [
        "anvil", "bullet", "cannon", "dagger", "ember", "flail", "glaive", "halberd", "rocket",
    ]
    .iter()
    .map(|name| entry(&format!("prefabs/{name}.kprefab"), "prefab"))
    .collect();
    assets.push(entry("sounds/alarm.wav", "audio"));
    assets
}

#[test]
fn reveal_in_assets_asks_the_browser_for_the_held_assets_path() {
    let inspected = armory(vec![(
        "bullet",
        optional(asset(AssetKind::Prefab)),
        ScriptValue::Asset(uuid(ROCKET)),
    )]);
    let mut state = EditorState {
        asset_entries: project(),
        asset_epoch: EPOCH,
        ..EditorState::default()
    };
    let (_, edits) = frame(
        &inspected,
        &mut state,
        Input {
            context_menu_open: true,
            clicks: clicking(&["Reveal in Assets"]),
            ..Input::default()
        },
    );
    assert!(edits.is_empty(), "revealing edits nothing: {edits:?}");
    assert_eq!(
        state.pending_reveal_asset.as_deref(),
        Some("prefabs/weapons/rocket.kprefab")
    );
}

/// The index writes a path as the platform does; the browser is asked for it
/// with forward slashes.
#[test]
fn reveal_in_assets_names_the_path_with_forward_slashes() {
    let mut assets = project();
    assets[ROCKET].source_path = "prefabs\\weapons\\rocket.kprefab".to_owned();
    let inspected = armory(vec![(
        "bullet",
        asset(AssetKind::Prefab),
        ScriptValue::Asset(uuid(ROCKET)),
    )]);
    let mut state = EditorState {
        asset_entries: assets,
        asset_epoch: EPOCH,
        ..EditorState::default()
    };
    frame(
        &inspected,
        &mut state,
        Input {
            context_menu_open: true,
            clicks: clicking(&["Reveal in Assets"]),
            ..Input::default()
        },
    );
    assert_eq!(
        state.pending_reveal_asset.as_deref(),
        Some("prefabs/weapons/rocket.kprefab")
    );
}

#[test]
fn a_picker_shows_only_the_entries_its_query_matches_whatever_their_case() {
    let inspected = armory(vec![(
        "bullet",
        optional(asset(AssetKind::Prefab)),
        ScriptValue::Null,
    )]);
    let mut state = EditorState {
        asset_entries: arsenal(),
        asset_epoch: EPOCH,
        asset_picker_query: "HALB".to_owned(),
        ..EditorState::default()
    };
    let (ui, edits) = frame(
        &inspected,
        &mut state,
        Input {
            click_menu_open: true,
            ..Input::default()
        },
    );
    assert!(edits.is_empty());
    assert!(ui.has_text_field(), "{:?}", ui.drawn());
    let buttons = ui.buttons();
    assert!(buttons.iter().any(|text| text == "halberd"), "{buttons:?}");
    for other in ["anvil", "bullet", "rocket"] {
        assert!(
            !buttons.iter().any(|text| text == other),
            "{other} does not match `HALB`: {buttons:?}"
        );
    }
}

/// The query typed in an open picker is still there the frame after; a pick
/// clears it.
#[test]
fn a_pickers_query_outlives_the_frame_until_a_pick_clears_it() {
    let inspected = armory(vec![(
        "bullet",
        optional(asset(AssetKind::Prefab)),
        ScriptValue::Null,
    )]);
    let mut state = EditorState {
        asset_entries: arsenal(),
        asset_epoch: EPOCH,
        asset_picker_query: "halb".to_owned(),
        ..EditorState::default()
    };
    let open = || Input {
        click_menu_open: true,
        ..Input::default()
    };
    frame(&inspected, &mut state, open());
    assert_eq!(
        state.asset_picker_query, "halb",
        "kept while nothing is picked"
    );

    let (_, edits) = frame(
        &inspected,
        &mut state,
        Input {
            clicks: clicking(&["halberd"]),
            ..open()
        },
    );
    let halberd = arsenal()[7].uuid;
    assert_eq!(
        edits,
        [format!(
            "{:?}",
            PropertyEdit::SetDeclaredField {
                entity: inspected.entity,
                component: "Armory".to_owned(),
                field: "bullet".to_owned(),
                value: ScriptValue::Asset(halberd),
            }
        )]
    );
    assert_eq!(state.asset_picker_query, "", "a pick clears the query");
}

/// A query left by one picker — typed, then dismissed without a pick — must
/// not hide the entries of the next picker opened, least of all one with no
/// search field to clear it.
#[test]
fn a_query_left_by_a_dismissed_picker_hides_nothing_from_a_picker_without_search() {
    let inspected = armory(vec![(
        "alarm",
        optional(asset(AssetKind::Sound)),
        ScriptValue::Null,
    )]);
    let mut state = EditorState {
        asset_entries: arsenal(),
        asset_epoch: EPOCH,
        // Typed into the nine-prefab picker of another slot, which was then
        // closed by a click outside.
        asset_picker_query: "halb".to_owned(),
        ..EditorState::default()
    };
    let (ui, _) = frame(
        &inspected,
        &mut state,
        Input {
            click_menu_open: true,
            ..Input::default()
        },
    );
    assert!(!ui.has_text_field(), "one sound needs no search field");
    assert!(
        ui.buttons().iter().any(|text| text == "alarm"),
        "the only sound of the project is offered: {:?}",
        ui.buttons()
    );
}

#[test]
fn choose_in_the_context_menu_picks_an_asset() {
    let mut value = ScriptValue::Asset(uuid(BULLET));
    let (_, changed) = draw(
        &optional(asset(AssetKind::Prefab)),
        &mut value,
        Input {
            context_menu_open: true,
            clicks: clicking(&["Choose", "rocket"]),
            ..Input::default()
        },
    );
    assert_eq!(value, ScriptValue::Asset(uuid(ROCKET)));
    assert!(changed);
}

#[test]
fn picking_or_dropping_the_asset_already_held_is_no_change() {
    for input in [
        Input {
            click_menu_open: true,
            clicks: clicking(&["bullet"]),
            ..Input::default()
        },
        dropping(dragged(BULLET)),
    ] {
        let mut value = ScriptValue::Asset(uuid(BULLET));
        let (_, changed) = draw(&asset(AssetKind::Prefab), &mut value, input);
        assert_eq!(value, ScriptValue::Asset(uuid(BULLET)));
        assert!(!changed);
    }
}

/// A slot inside a struct field, and an item of an asset list, see the
/// project's assets as the card's own slots do.
#[test]
fn a_slot_within_a_struct_or_a_list_takes_a_drop_of_its_kind() {
    let loadout = ErgonType::Struct {
        name: "Loadout".to_owned(),
        fields: Some(vec![(
            "shell".to_owned(),
            optional(asset(AssetKind::Prefab)),
        )]),
    };
    let mut value = ScriptValue::Struct(vec![("shell".to_owned(), ScriptValue::Null)]);
    let (_, changed) = draw(&loadout, &mut value, dropping(dragged(ROCKET)));
    assert_eq!(
        value,
        ScriptValue::Struct(vec![("shell".to_owned(), ScriptValue::Asset(uuid(ROCKET)))])
    );
    assert!(changed);

    let list = ErgonType::Array(Box::new(asset(AssetKind::Prefab)));
    let mut value = ScriptValue::Array(vec![ScriptValue::Asset(uuid(BULLET))]);
    let (_, changed) = draw(&list, &mut value, dropping(dragged(ROCKET)));
    assert_eq!(
        value,
        ScriptValue::Array(vec![ScriptValue::Asset(uuid(ROCKET))])
    );
    assert!(changed);
}

/// Two assets of one name in two folders are both offered, each with its
/// folder.
#[test]
fn a_picker_tells_apart_two_assets_of_one_name_by_their_folders() {
    let assets = vec![
        entry("enemies/bullet.kprefab", "prefab"),
        entry("player/bullet.kprefab", "prefab"),
    ];
    let theme = UiTheme::default();
    let names = HashMap::new();
    let cx = FieldContext {
        theme: &theme,
        names: &names,
        id: "declared.1.Armory.bullet".to_owned(),
        range: None,
        tooltip: None,
        assets: &assets,
        asset_epoch: EPOCH,
        memory: None,
    };
    let mut ui = Spy::with(Input {
        click_menu_open: true,
        ..Input::default()
    });
    render_field(
        &mut ui,
        "bullet",
        &optional(asset(AssetKind::Prefab)),
        &mut ScriptValue::Null,
        false,
        &cx,
    );
    let texts: Vec<String> = ui.texts().into_iter().map(|(text, _)| text).collect();
    assert_eq!(
        ui.buttons().iter().filter(|text| *text == "bullet").count(),
        2,
        "{:?}",
        ui.buttons()
    );
    for folder in ["enemies", "player"] {
        assert!(texts.iter().any(|text| text == folder), "{texts:?}");
    }
}

/// The context menu's "Choose…" closes on any click inside it, so it can
/// hold no search field: it lists every entry of the kind, and a query left
/// by another picker hides none of them.
#[test]
fn choose_lists_every_entry_of_its_kind_with_no_search_field() {
    let inspected = armory(vec![(
        "bullet",
        optional(asset(AssetKind::Prefab)),
        ScriptValue::Null,
    )]);
    let mut state = EditorState {
        asset_entries: arsenal(),
        asset_epoch: EPOCH,
        asset_picker_query: "halb".to_owned(),
        ..EditorState::default()
    };
    let (ui, edits) = frame(
        &inspected,
        &mut state,
        Input {
            context_menu_open: true,
            clicks: clicking(&["Choose"]),
            ..Input::default()
        },
    );
    assert!(edits.is_empty());
    assert!(!ui.has_text_field(), "{:?}", ui.drawn());
    let buttons = ui.buttons();
    for name in [
        "anvil", "bullet", "cannon", "dagger", "ember", "flail", "glaive", "halberd", "rocket",
    ] {
        assert!(
            buttons.iter().any(|text| text == name),
            "{name} is offered: {buttons:?}"
        );
    }
    assert_eq!(
        state.asset_picker_query, "halb",
        "the query of the searchable picker is kept for it"
    );
}

/// The "+ Add item" picker of a long asset list searches as the slot's own
/// picker does, and pushes what is picked from the filtered entries.
#[test]
fn the_add_item_picker_of_a_long_list_searches_and_pushes_the_pick() {
    let inspected = armory(vec![(
        "arsenal",
        ErgonType::Array(Box::new(asset(AssetKind::Prefab))),
        ScriptValue::Array(Vec::new()),
    )]);
    let mut state = EditorState {
        asset_entries: arsenal(),
        asset_epoch: EPOCH,
        asset_picker_query: "halb".to_owned(),
        ..EditorState::default()
    };
    let (ui, edits) = frame(
        &inspected,
        &mut state,
        Input {
            click_menu_open: true,
            ..Input::default()
        },
    );
    assert!(edits.is_empty(), "an open picker adds nothing: {edits:?}");
    assert!(ui.has_text_field(), "{:?}", ui.drawn());
    assert!(
        !ui.buttons().iter().any(|text| text == "anvil"),
        "`halb` hides anvil: {:?}",
        ui.buttons()
    );

    let (_, edits) = frame(
        &inspected,
        &mut state,
        Input {
            click_menu_open: true,
            clicks: clicking(&["halberd"]),
            ..Input::default()
        },
    );
    let halberd = arsenal()[7].uuid;
    assert_eq!(
        edits,
        [format!(
            "{:?}",
            PropertyEdit::SetDeclaredField {
                entity: inspected.entity,
                component: "Armory".to_owned(),
                field: "arsenal".to_owned(),
                value: ScriptValue::Array(vec![ScriptValue::Asset(halberd)]),
            }
        )]
    );
    assert_eq!(state.asset_picker_query, "", "a pick clears the query");
}

/// A project of `count` textures.
fn gallery(count: usize) -> Vec<AssetEntry> {
    (0..count)
        .map(|i| entry(&format!("textures/t{i:03}.png"), "texture"))
        .collect()
}

/// A picker taller than the window must scroll: a menu laid out past the
/// window's edge is cut there (egui keeps a menu inside the screen), and the
/// "Choose…" submenu has no search field to narrow it. Sixty entries of
/// 20 px do not fit a window.
#[test]
fn a_long_picker_scrolls_its_entries_in_either_menu() {
    let inspected = armory(vec![(
        "skin",
        optional(asset(AssetKind::Texture)),
        ScriptValue::Null,
    )]);
    for (menu, input) in [
        (
            "the click picker",
            Input {
                click_menu_open: true,
                ..Input::default()
            },
        ),
        (
            "Choose…",
            Input {
                context_menu_open: true,
                clicks: clicking(&["Choose"]),
                ..Input::default()
            },
        ),
    ] {
        let mut state = EditorState {
            asset_entries: gallery(60),
            asset_epoch: EPOCH,
            ..EditorState::default()
        };
        let (ui, _) = frame(&inspected, &mut state, input);
        let drawn = ui.drawn();
        let last = drawn
            .iter()
            .position(|item| *item == Drawn::Button("t059".to_owned()));
        let scroll = drawn.iter().position(|item| *item == Drawn::Scroll);
        assert!(
            last.is_some(),
            "{menu} offers every texture: {:?}",
            ui.buttons()
        );
        assert!(
            scroll.is_some_and(|scroll| Some(scroll) < last),
            "{menu}: sixty textures are laid out with no scroll area"
        );
    }
}
