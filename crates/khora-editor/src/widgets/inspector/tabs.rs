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

use khora_sdk::editor_ui::{EditorState, UiTheme, InspectedEntity, PropertyEdit, UiBuilder};
use khora_sdk::prelude::ecs::EntityId;
use khora_sdk::CommandHistory;

use super::add_component::{render_add_component, INHERENT_COMPONENTS};
use super::card::render_card;
use super::display::icon_for_domain_tag;
use super::walker::render_value;

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
    fn id(&self) -> &'static str;
    fn label(&self) -> &str;
    fn render(&mut self, ui: &mut dyn UiBuilder, body_rect: [f32; 4], ctx: &mut InspectorTabContext<'_>);
}

/// Default tab — component cards plus "+ Add Component" menu.
pub struct PropertiesTab;

impl InspectorTab for PropertiesTab {
    fn id(&self) -> &'static str {
        "properties"
    }
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

        ui.region_at(body_rect, &mut |ui_inner| {
            let mut state_guard = match state_arc.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            let panel_rect_inner = ui_inner.panel_rect();
            let card_x = panel_rect_inner[0];
            let card_w = panel_rect_inner[2];

            let mut edits: Vec<PropertyEdit> = Vec::new();

            for cj in inspected.components_json.iter() {
                if INHERENT_COMPONENTS.contains(&cj.type_name.as_str()) {
                    continue;
                }
                let title = cj.type_name.clone();
                let mut value = cj.value.clone();
                let icon = icon_for_domain_tag(cj.domain);
                let mut changed = false;
                render_card(
                    ui_inner,
                    entity,
                    &title,
                    icon,
                    None,
                    true, // removable — INHERENT_COMPONENTS already filtered
                    card_x,
                    card_w,
                    &theme,
                    &mut state_guard,
                    &mut |ui_b| {
                        changed = render_value(ui_b, &mut value, &theme);
                    },
                );
                if changed {
                    edits.push(PropertyEdit::SetComponentJson {
                        entity,
                        type_name: cj.type_name.clone(),
                        value,
                    });
                }
            }

            ui_inner.spacing(8.0);
            render_add_component(ui_inner, entity, &inspected, &mut state_guard);

            for e in edits.drain(..) {
                state_guard.push_edit(e);
            }
        });
    }
}

/// Debug tab — undo / redo stack inspection.
pub struct DebugTab;

impl InspectorTab for DebugTab {
    fn id(&self) -> &'static str {
        "debug"
    }
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
        ui.region_at(body_rect, &mut |ui_inner| {
            ui_inner.colored_label(theme.text_dim, "Command history:");
            ui_inner.colored_label(
                theme.text_muted,
                &format!("Undo: {} | Redo: {}", undo_desc, redo_desc),
            );
        });
    }
}
