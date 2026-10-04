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

//! The instance band: one quiet line under the inspector header saying which
//! prefab the entity's instance comes from, with the instance-wide actions.
//!
//! A line, not a card — the inspector's structure comes from type and
//! spacing. The prefab's name is the band's one link: it selects the prefab
//! in the asset browser, as a reference does anywhere else in the editor.

use khora_sdk::editor_ui::{
    EditorState, FontFamilyHint, Icon, InspectedPrefab, PrefabApplyScope, UiBuilder, UiTheme,
};
use khora_sdk::prelude::ecs::EntityId;
use khora_tool_ui::widgets::paint::{icon, text, tint};
use khora_tool_ui::widgets::{self};

const BAND_H: f32 = 30.0;

/// The band for `entity`, a part of the instance `prefab` describes.
pub fn render_instance_band(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    prefab: &InspectedPrefab,
    x: f32,
    width: f32,
    theme: &UiTheme,
    state: &mut EditorState,
) {
    let y = ui.cursor_pos()[1];
    let size = theme.font_size_caption;
    let text_y = y + (BAND_H - size) * 0.5 - 1.0;

    icon(
        ui,
        [x + 12.0, y + (BAND_H - 13.0) * 0.5],
        Icon::Prefab,
        13.0,
        theme.primary_dim,
    );
    let lead = if prefab.is_root {
        "Instance of"
    } else {
        "Part of an instance of"
    };
    let lead_x = x + 32.0;
    text(ui, [lead_x, text_y], lead, size, theme.text_dim);
    let lead_w = ui.measure_text(lead, size, FontFamilyHint::Proportional)[0];

    // The prefab's file name, the band's link to it.
    let name = prefab
        .prefab_path
        .as_deref()
        .map(|path| path.rsplit('/').next().unwrap_or(path).to_owned())
        .unwrap_or_else(|| "its prefab".to_owned());
    let name_x = lead_x + lead_w + 6.0;
    let name_w = ui.measure_text(&name, size, FontFamilyHint::Proportional)[0];
    let name_rect = [name_x - 2.0, y + 4.0, name_w + 4.0, BAND_H - 8.0];
    let link = ui.interact_rect(&format!("prefab-link-{}", entity.index), name_rect);
    let link_color = if link.hovered {
        theme.text
    } else {
        theme.primary
    };
    text(ui, [name_x, text_y], &name, size, link_color);
    if link.hovered {
        let underline = text_y + size + 2.0;
        ui.paint_line(
            [name_x, underline],
            [name_x + name_w, underline],
            link_color,
            1.0,
        );
    }
    if link.clicked {
        select_prefab_asset(prefab, state);
    }

    // The instance-wide actions, right-aligned as words: rare, and too
    // consequential to sit behind an icon nobody can read.
    let mut right = x + width - 10.0;
    for (label, apply) in [("Apply all", true), ("Revert all", false)] {
        let w = ui.measure_text(label, size, FontFamilyHint::Proportional)[0] + 14.0;
        let rect = [right - w, y + 5.0, w, BAND_H - 10.0];
        right -= w + 4.0;
        let hit = ui.interact_rect(&format!("prefab-{label}-{}", entity.index), rect);
        if hit.hovered {
            widgets::fill(ui, rect, tint(theme.primary, 0.14), theme.radius_sm);
        }
        text(
            ui,
            [rect[0] + 7.0, text_y],
            label,
            size,
            if hit.hovered {
                theme.text
            } else {
                theme.text_dim
            },
        );
        if hit.clicked {
            if apply {
                state.pending_prefab_apply = Some((entity, PrefabApplyScope::Instance));
            } else {
                state.pending_prefab_revert = Some(entity);
            }
        }
    }

    ui.spacing(BAND_H);
    let end_y = ui.cursor_pos()[1];
    ui.paint_line([x, end_y], [x + width, end_y], theme.separator, 1.0);
    ui.spacing(6.0);
}

/// Selects the prefab in the asset browser and shows it in the inspector.
fn select_prefab_asset(prefab: &InspectedPrefab, state: &mut EditorState) {
    let Some(path) = prefab.prefab_path.clone() else {
        return;
    };
    state.selection.clear();
    state.inspected = None;
    state.selected_asset = state
        .asset_entries
        .iter()
        .position(|entry| entry.source_path.replace('\\', "/").ends_with(&path));
    state.inspected_asset_path = Some(path);
}
