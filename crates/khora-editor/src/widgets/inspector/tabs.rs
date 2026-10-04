// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Inspector tabs — `Properties` and `Debug`. Adding a third tab is a
//! matter of implementing [`InspectorTab`] and adding it to the panel's
//! `tabs` vector at construction time.
//!
//! `Events` and `Prefab` tabs are intentionally absent — see the editor
//! design doc.

use std::sync::{Arc, Mutex};

use crate::commands::CommandHistory;
use khora_sdk::editor_ui::{
    EditorState, FontFamilyHint, Icon, InspectedEntity, InspectedPrefab, PrefabApplyScope,
    PropertyEdit, UiBuilder, UiTheme,
};
use khora_sdk::prelude::ecs::EntityId;
use khora_tool_ui::widgets::paint::{icon, text, tint};
use khora_tool_ui::widgets::{self};

use crate::ops::prefab_overrides::reverted;

use super::add_component::{is_author_facing, render_add_component};
use super::card::{render_card, CardPrefab};
use super::display::icon_for_domain_tag;
use super::instance_band::render_instance_band;
use super::tag_chips::render_tag_chips;
use super::walker::{render_value_marked, FieldAction, Marks};

/// Per-tab context handed to [`InspectorTab::render`]. Locked once by the
/// caller and passed through.
pub struct InspectorTabContext<'a> {
    pub state: &'a Arc<Mutex<EditorState>>,
    pub history: &'a Arc<Mutex<CommandHistory>>,
    pub theme: &'a UiTheme,
    pub entity: EntityId,
    pub inspected: &'a InspectedEntity,
}

/// A self-contained body for one Inspector sub-tab. The panel iterates
/// the registered tabs and dispatches the active one's `render`.
pub trait InspectorTab: Send + Sync {
    fn label(&self) -> &str;
    fn render(
        &mut self,
        ui: &mut dyn UiBuilder,
        body_rect: [f32; 4],
        ctx: &mut InspectorTabContext<'_>,
    );
}

/// Default tab — component cards plus "+ Add Component" menu.
pub struct PropertiesTab;

impl InspectorTab for PropertiesTab {
    fn label(&self) -> &str {
        "Properties"
    }

    fn render(
        &mut self,
        ui: &mut dyn UiBuilder,
        body_rect: [f32; 4],
        ctx: &mut InspectorTabContext<'_>,
    ) {
        let theme = ctx.theme.clone();
        let entity = ctx.entity;
        let inspected = ctx.inspected.clone();
        let state_arc = ctx.state.clone();

        // Component cards flow with the egui cursor rather than being placed at
        // absolute rects, so the stock scroll area is the right tool here —
        // unlike the Console and the Hierarchy, which paint into computed rects
        // and use the cursor-offset helper instead.
        ui.region_at("inspector-properties", body_rect, &mut |ui_region| {
            ui_region.scroll_area("inspector-cards", &mut |ui_inner| {
                let mut state_guard = match state_arc.lock() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let panel_rect_inner = ui_inner.panel_rect();
                let card_x = panel_rect_inner[0];
                let card_w = panel_rect_inner[2];

                let mut edits: Vec<PropertyEdit> = Vec::new();
                let prefab = inspected.prefab.as_ref();
                if let Some(prefab) = prefab {
                    render_instance_band(
                        ui_inner,
                        entity,
                        prefab,
                        card_x,
                        card_w,
                        &theme,
                        &mut state_guard,
                    );
                }

                for cj in inspected.components_json.iter() {
                    if !is_author_facing(&cj.type_name) {
                        continue;
                    }
                    let title = cj.type_name.clone();
                    let mut value = cj.value.clone();
                    let icon = icon_for_domain_tag(cj.domain);
                    let mut changed = false;
                    let overridden: Vec<Vec<String>> = prefab
                        .map(|prefab| prefab.fields_of(&cj.type_name).to_vec())
                        .unwrap_or_default();
                    let prefab_value = prefab.and_then(|prefab| prefab_value_of(prefab, &title));
                    let card_prefab = prefab.map(|prefab| CardPrefab {
                        overrides: overridden.len(),
                        added: prefab
                            .components
                            .iter()
                            .any(|component| component.type_name == title && component.added),
                        prefab_value,
                    });
                    let mut marks = Marks::new(&overridden);
                    render_card(
                        ui_inner,
                        entity,
                        &title,
                        icon,
                        None,
                        true, // removable — `is_author_facing` already filtered
                        card_prefab,
                        card_x,
                        card_w,
                        &theme,
                        &mut state_guard,
                        &mut |ui_b| {
                            // Tag has a custom chip renderer — the generic JSON
                            // walker would render it as `[0] = "alpha", …` rows.
                            if cj.type_name == "Tag" {
                                changed = render_tag_chips(ui_b, entity, &mut value);
                            } else {
                                changed = render_value_marked(ui_b, &mut value, &theme, &mut marks);
                            }
                        },
                    );
                    if changed {
                        edits.push(PropertyEdit::SetComponentJson {
                            entity,
                            type_name: cj.type_name.clone(),
                            value,
                        });
                    }
                    for action in marks.actions.drain(..) {
                        match action {
                            FieldAction::Revert(path) => {
                                if let Some(prefab_value) = prefab_value {
                                    edits.push(PropertyEdit::SetComponentJson {
                                        entity,
                                        type_name: title.clone(),
                                        value: reverted(&cj.value, prefab_value, &path),
                                    });
                                }
                            }
                            FieldAction::Apply(path) => {
                                state_guard.pending_prefab_apply = Some((
                                    entity,
                                    PrefabApplyScope::Field {
                                        type_name: title.clone(),
                                        path,
                                    },
                                ));
                            }
                        }
                    }
                }

                // What the prefab gives the entity and the instance took off:
                // a dim line each, with the way back.
                if let Some(prefab) = prefab {
                    for type_name in prefab.removed.iter().filter(|name| is_author_facing(name)) {
                        if let Some(edit) = render_removed_component(
                            ui_inner, entity, prefab, type_name, card_x, card_w, &theme,
                        ) {
                            edits.push(edit);
                        }
                    }
                }

                ui_inner.spacing(8.0);
                render_add_component(ui_inner, entity, &inspected, &mut state_guard);

                for e in edits.drain(..) {
                    state_guard.push_edit(e);
                }
            });
        });
    }
}

/// The prefab's value of the component `type_name`.
fn prefab_value_of<'a>(
    prefab: &'a InspectedPrefab,
    type_name: &str,
) -> Option<&'a serde_json::Value> {
    prefab
        .prefab_json
        .iter()
        .find(|(name, _)| name == type_name)
        .map(|(_, value)| value)
}

/// One component the prefab gives the entity and the instance removed: a dim
/// line that says so, and restores it on demand. The edit, when asked.
#[allow(clippy::too_many_arguments)]
fn render_removed_component(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    prefab: &InspectedPrefab,
    type_name: &str,
    x: f32,
    width: f32,
    theme: &UiTheme,
) -> Option<PropertyEdit> {
    const ROW_H: f32 = 28.0;
    let y = ui.cursor_pos()[1];
    let size = theme.font_size_body;
    let text_y = y + (ROW_H - size) * 0.5 - 1.0;
    icon(
        ui,
        [x + 12.0, y + (ROW_H - 12.0) * 0.5],
        Icon::Prefab,
        12.0,
        theme.text_disabled,
    );
    text(ui, [x + 32.0, text_y], type_name, size, theme.text_disabled);
    let name_w = ui.measure_text(type_name, size, FontFamilyHint::Proportional)[0];
    let note_size = theme.font_size_caption - 1.5;
    ui.paint_text_styled(
        [x + 40.0 + name_w, y + (ROW_H - note_size) * 0.5 - 1.0],
        "removed",
        note_size,
        theme.primary,
        FontFamilyHint::Monospace,
        khora_sdk::editor_ui::TextAlign::Left,
    );

    let label = "Restore";
    let w = ui.measure_text(label, theme.font_size_caption, FontFamilyHint::Proportional)[0] + 14.0;
    let rect = [x + width - 10.0 - w, y + 4.0, w, ROW_H - 8.0];
    let hit = ui.interact_rect(&format!("restore-{}-{type_name}", entity.index), rect);
    if hit.hovered {
        widgets::fill(ui, rect, tint(theme.primary, 0.14), theme.radius_sm);
    }
    text(
        ui,
        [
            rect[0] + 7.0,
            y + (ROW_H - theme.font_size_caption) * 0.5 - 1.0,
        ],
        label,
        theme.font_size_caption,
        if hit.hovered {
            theme.text
        } else {
            theme.text_dim
        },
    );
    ui.spacing(ROW_H);
    let end_y = ui.cursor_pos()[1];
    ui.paint_line([x, end_y], [x + width, end_y], theme.separator, 1.0);
    ui.spacing(6.0);

    let value = prefab_value_of(prefab, type_name)?.clone();
    hit.clicked.then(|| PropertyEdit::InsertComponentJson {
        entity,
        type_name: type_name.to_owned(),
        value,
    })
}

/// Debug tab — undo / redo stack inspection.
pub struct DebugTab;

impl InspectorTab for DebugTab {
    fn label(&self) -> &str {
        "Debug"
    }

    fn render(
        &mut self,
        ui: &mut dyn UiBuilder,
        body_rect: [f32; 4],
        ctx: &mut InspectorTabContext<'_>,
    ) {
        let theme = ctx.theme.clone();
        let history = ctx.history.clone();
        let undo_desc = history
            .lock()
            .ok()
            .and_then(|h| h.undo_description().map(|s| s.to_owned()))
            .unwrap_or_else(|| "(none)".to_owned());
        let redo_desc = history
            .lock()
            .ok()
            .and_then(|h| h.redo_description().map(|s| s.to_owned()))
            .unwrap_or_else(|| "(none)".to_owned());
        ui.region_at("inspector-debug", body_rect, &mut |ui_inner| {
            ui_inner.colored_label(theme.text_dim, "Command history:");
            ui_inner.colored_label(
                theme.text_muted,
                &format!("Undo: {} | Redo: {}", undo_desc, redo_desc),
            );
        });
    }
}
