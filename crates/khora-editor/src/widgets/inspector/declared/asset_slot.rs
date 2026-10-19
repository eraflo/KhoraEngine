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

//! An asset slot: the twin of the entity slot, for a field typed by a kind
//! of asset — `Prefab? bullet;`.
//!
//! The slot names what it holds by the file it lives in, takes an asset of
//! its kind dragged from the browser, and opens a picker of that kind on a
//! click. A drag it would refuse leaves it as it was: no light, no message.

use std::cell::RefCell;

use khora_sdk::editor_ui::{AssetEntry, FontFamilyHint, Icon, TextAlign, UiBuilder};
use khora_sdk::khora_core::asset::{AssetKind, AssetUUID};
use khora_sdk::khora_core::script::ScriptValue;
use khora_tool_ui::widgets::paint::tint;
use khora_tool_ui::widgets::{self};

use super::fields::FieldContext;
use super::ranged::render_label;
use crate::drag_payload::unpack_asset_drag;
use crate::widgets::asset_kind::{asset_kind_icon, asset_kind_noun};

/// The height of a slot's well.
const SLOT_H: f32 = 22.0;

/// A picker of more entries than this has a search field.
const SEARCH_FROM: usize = 8;

/// What the asset slots of a frame leave for the frame after it.
#[derive(Debug, Default)]
pub struct SlotMemory {
    /// What the open picker filters its entries by.
    pub query: String,
    /// The path of the asset the author asked to reveal in the browser.
    pub reveal: Option<String>,
}

/// An asset as a slot names it: the file stem of the entry holding `uuid`,
/// or `missing · ` and the first eight hex digits of `uuid` where no entry
/// holds it.
pub fn asset_text(uuid: AssetUUID, entries: &[AssetEntry]) -> String {
    match entries.iter().find(|entry| entry.uuid == uuid) {
        Some(entry) => stem(&entry.name).to_owned(),
        None => missing_text(uuid),
    }
}

/// The entries a picker for `kind` offers: those of that kind, sorted by
/// name.
pub fn picker_entries(kind: AssetKind, entries: &[AssetEntry]) -> Vec<&AssetEntry> {
    let mut offered: Vec<&AssetEntry> = entries
        .iter()
        .filter(|entry| AssetKind::from_index_name(&entry.asset_type) == Some(kind))
        .collect();
    offered.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.source_path.cmp(&b.source_path))
    });
    offered
}

/// Draws the slot of `label`, holding an asset of `kind` or none; returns
/// whether the author changed it. An `optional` one is cleared with its
/// context menu.
pub fn render_asset(
    ui: &mut dyn UiBuilder,
    label: &str,
    kind: AssetKind,
    value: &mut ScriptValue,
    optional: bool,
    cx: &FieldContext<'_>,
) -> bool {
    let theme = cx.theme;
    let held = match value {
        ScriptValue::Asset(uuid) => Some(*uuid),
        _ => None,
    };
    let present = held.and_then(|uuid| cx.assets.iter().find(|entry| entry.uuid == uuid));
    let noun = asset_kind_noun(kind);
    let mut chosen: Option<AssetUUID> = None;
    let mut cleared = false;
    let mut revealed = false;
    ui.horizontal(&mut |row| {
        render_label(row, cx, label, theme.text);
        let width = row.available_width().max(80.0);
        let rect = row.allocate_size([width, SLOT_H]);
        let hit = row.interact_rect(&format!("{}.asset-slot", cx.id), rect);
        let accepts = row
            .dnd_hover_payload()
            .is_some_and(|payload| dragged_entry(payload, kind, cx).is_some());
        let fill = if accepts {
            tint(theme.primary, 0.18)
        } else if hit.hovered {
            theme.surface_active
        } else {
            theme.surface_interactive
        };
        widgets::fill(row, rect, fill, theme.radius_sm);
        if accepts {
            row.paint_rect_stroke(
                [rect[0], rect[1]],
                [rect[2], rect[3]],
                theme.primary,
                theme.radius_sm,
                1.0,
            );
        }
        paint_contents(row, rect, kind, held, present, cx);
        match (held, present) {
            (Some(_), Some(entry)) => row.tooltip_for_last(&display_path(&entry.source_path)),
            (Some(_), None) => row.tooltip_for_last(&format!(
                "This {noun} is no longer in the project.\nPick another, or set to none."
            )),
            (None, _) => {}
        }
        // Whatever is released on the slot is its own: a drop of another
        // kind is taken, and changes nothing.
        if let Some(payload) = row.dnd_take_drop_payload() {
            if let Some(entry) = dragged_entry(payload, kind, cx) {
                chosen = Some(entry.uuid);
            }
        }
        row.menu_on_click_last(&mut |menu| {
            if let Some(uuid) = pick_asset(menu, kind, held, true, cx) {
                chosen = Some(uuid);
                menu.close_menu();
            }
        });
        row.context_menu_last(&mut |menu| {
            // A context menu closes on any click inside: its picker has no
            // search field to click into.
            menu.menu_button("Choose…", &mut |choices| {
                if let Some(uuid) = pick_asset(choices, kind, held, false, cx) {
                    chosen = Some(uuid);
                    choices.close_menu();
                }
            });
            if present.is_some() && menu.button("Reveal in Assets") {
                revealed = true;
                menu.close_menu();
            }
            if optional && held.is_some() && menu.button("Set to none") {
                cleared = true;
                menu.close_menu();
            }
        });
    });
    if revealed {
        if let (Some(memory), Some(entry)) = (cx.memory, present) {
            memory.borrow_mut().reveal = Some(display_path(&entry.source_path));
        }
    }
    if let Some(uuid) = chosen {
        let picked = ScriptValue::Asset(uuid);
        if *value == picked {
            return false;
        }
        *value = picked;
        return true;
    }
    if cleared {
        *value = ScriptValue::Null;
        return true;
    }
    false
}

/// The picker of `kind`: its entries by name, each with its folder muted and
/// its path as a tooltip, and — when `searchable` and there are many — a
/// search field filtering them. Returns the asset picked, if any.
pub fn pick_asset(
    ui: &mut dyn UiBuilder,
    kind: AssetKind,
    held: Option<AssetUUID>,
    searchable: bool,
    cx: &FieldContext<'_>,
) -> Option<AssetUUID> {
    let theme = cx.theme;
    let offered = picker_entries(kind, cx.assets);
    if offered.is_empty() {
        ui.colored_label(
            theme.text_muted,
            &format!("No {} in the project", asset_kind_noun(kind)),
        );
        return None;
    }
    // Without a memory to keep it in, the query lasts the frame.
    let scratch = RefCell::new(SlotMemory::default());
    let memory = cx.memory.unwrap_or(&scratch);
    let mut query = std::mem::take(&mut memory.borrow_mut().query);
    // A query filters only where its field shows it: one left by another
    // picker never hides what this one offers.
    let searching = searchable && offered.len() > SEARCH_FROM;
    if searching {
        ui.text_edit_singleline(&mut query);
    }
    let needle = if searching {
        query.to_lowercase()
    } else {
        String::new()
    };
    let mut picked = None;
    // A menu is held inside the window and does not scroll: the entries do,
    // within what the window leaves below the menu's top.
    ui.scroll_area(&format!("{}.picker", cx.id), &mut |list| {
        for entry in offered.iter().filter(|entry| {
            needle.is_empty()
                || display_path(&entry.source_path)
                    .to_lowercase()
                    .contains(&needle)
        }) {
            list.horizontal(&mut |row| {
                if row.selectable_label(held == Some(entry.uuid), stem(&entry.name)) {
                    picked = Some(entry.uuid);
                }
                row.tooltip_for_last(&display_path(&entry.source_path));
                let folder = folder_of(&entry.source_path);
                if !folder.is_empty() {
                    row.colored_label(theme.text_muted, &folder);
                }
            });
        }
    });
    if picked.is_some() {
        query.clear();
    }
    memory.borrow_mut().query = query;
    picked
}

/// The icon and the words in the well: the asset's name and its folder, the
/// kind asked for, or the asset gone from the project.
fn paint_contents(
    ui: &mut dyn UiBuilder,
    rect: [f32; 4],
    kind: AssetKind,
    held: Option<AssetUUID>,
    present: Option<&AssetEntry>,
    cx: &FieldContext<'_>,
) {
    let theme = cx.theme;
    let size = theme.font_size_body;
    let y = rect[1] + (SLOT_H - size) * 0.5 - 1.0;
    let (icon, icon_color) = match (held, present) {
        (Some(_), None) => (Icon::Warn, theme.warning),
        (Some(_), Some(_)) => (asset_kind_icon(kind), theme.text_dim),
        (None, _) => (asset_kind_icon(kind), theme.text_muted),
    };
    ui.paint_text_styled(
        [rect[0] + 8.0, y],
        icon.glyph(),
        size,
        icon_color,
        FontFamilyHint::Icons,
        TextAlign::Left,
    );
    let x = rect[0] + 28.0;
    ui.push_clip_rect([rect[0], rect[1], rect[2] - 6.0, rect[3]]);
    match (held, present) {
        (Some(_), Some(entry)) => {
            let name = stem(&entry.name);
            paint_line(ui, [x, y], name, size, theme.text);
            let folder = folder_of(&entry.source_path);
            if !folder.is_empty() {
                let caption = theme.font_size_caption;
                let after = ui.measure_text(name, size, FontFamilyHint::Proportional)[0];
                let folder_y = rect[1] + (SLOT_H - caption) * 0.5 - 1.0;
                paint_line(
                    ui,
                    [x + after + 8.0, folder_y],
                    &folder,
                    caption,
                    theme.text_muted,
                );
            }
        }
        (Some(uuid), None) => paint_line(ui, [x, y], &missing_text(uuid), size, theme.warning),
        (None, _) => paint_line(
            ui,
            [x, y],
            &format!("none — drop a {}", asset_kind_noun(kind)),
            size,
            theme.text_muted,
        ),
    }
    ui.pop_clip_rect();
}

fn paint_line(ui: &mut dyn UiBuilder, at: [f32; 2], text: &str, size: f32, color: [f32; 4]) {
    ui.paint_text_styled(
        at,
        text,
        size,
        color,
        FontFamilyHint::Proportional,
        TextAlign::Left,
    );
}

/// The entry of `cx`'s list a drag of `payload` carries, where it is an
/// asset of `kind` dragged from that very list.
fn dragged_entry<'a>(
    payload: u64,
    kind: AssetKind,
    cx: &FieldContext<'a>,
) -> Option<&'a AssetEntry> {
    let index = unpack_asset_drag(payload, cx.asset_epoch)?;
    cx.assets
        .get(index as usize)
        .filter(|entry| AssetKind::from_index_name(&entry.asset_type) == Some(kind))
}

/// `missing · ` and the first group of `uuid`.
fn missing_text(uuid: AssetUUID) -> String {
    let text = uuid.to_string();
    format!("missing · {}", &text[..8])
}

/// A file name without its extension; a name that is all extension stays
/// whole.
fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// A path under `assets/` with forward slashes, whatever the platform wrote.
fn display_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// The folder holding the asset at `path`; empty at the root of `assets/`.
fn folder_of(path: &str) -> String {
    let path = display_path(path);
    match path.rsplit_once('/') {
        Some((folder, _)) => folder.to_owned(),
        None => String::new(),
    }
}
