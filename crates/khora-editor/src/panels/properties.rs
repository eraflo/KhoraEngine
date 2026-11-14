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

//! The Inspector panel: its chrome and header, over the inspector tabs and
//! the asset pane of `widgets::inspector`.

use crate::commands::CommandHistory;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::inspector::asset_pane::{paint_asset_header, render_asset_pane};
use crate::widgets::inspector::display::{pick_icon, pick_type_tag};
use crate::widgets::inspector::header::paint_inspector_header;
use crate::widgets::inspector::tabs::{DebugTab, InspectorTab, InspectorTabContext, PropertiesTab};
use crate::widgets::panel_header::paint_panel_header;

const HEADER_HEIGHT: f32 = 34.0;
const INSPECTOR_HEADER_HEIGHT: f32 = 64.0;
const SUBTAB_HEIGHT: f32 = 28.0;

/// The Inspector: the selected entity's components, or the selected asset.
pub struct PropertiesPanel {
    state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
    theme: UiTheme,
    tabs: Vec<Box<dyn InspectorTab>>,
    active_tab: usize,
}

impl PropertiesPanel {
    /// Returns the Inspector over the editor state and its undo history.
    pub fn new(
        state: Arc<Mutex<EditorState>>,
        history: Arc<Mutex<CommandHistory>>,
        theme: UiTheme,
    ) -> Self {
        let tabs: Vec<Box<dyn InspectorTab>> = vec![Box::new(PropertiesTab), Box::new(DebugTab)];
        Self {
            state,
            command_history: history,
            theme,
            tabs,
            active_tab: 0,
        }
    }
}

impl EditorPanel for PropertiesPanel {
    fn id(&self) -> &str {
        "khora.editor.properties"
    }
    fn title(&self) -> &str {
        "Inspector"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(320.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, _] = panel_rect;

        // No title chip: the dock tab above already names this panel.
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, &theme);

        // An asset selected in the browser shows its pane, unless an entity
        // is inspected.
        let (entity_data, asset_path, project_folder, quarantine) = {
            let s = match self.state.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            let entity_data = s.single_selected().and_then(|e| {
                s.inspected
                    .clone()
                    .filter(|i| i.entity == e)
                    .map(|i| (e, i))
            });
            let quarantine = s.inspected_asset_path.as_ref().and_then(|rel| {
                (s.quarantined.iter())
                    .find(|asset| &asset.rel_path == rel)
                    .cloned()
            });
            (
                entity_data,
                s.inspected_asset_path.clone(),
                s.project_folder.clone().unwrap_or_default(),
                quarantine,
            )
        };
        let asset_mode = entity_data.is_none() && asset_path.is_some();

        let header_origin = [px, py + HEADER_HEIGHT];
        let after_header = if asset_mode {
            paint_asset_header(ui, header_origin, pw, asset_path.as_ref().unwrap(), &theme)
        } else {
            match &entity_data {
                Some((entity, inspected)) => {
                    let icon = pick_icon(inspected);
                    let id_label = format!("id 0x{:04X}", entity.index);
                    let type_tag = pick_type_tag(inspected);
                    paint_inspector_header(
                        ui,
                        header_origin,
                        pw,
                        icon,
                        &inspected.name,
                        type_tag,
                        "Active",
                        theme.success,
                        Some(&id_label),
                        &theme,
                    )
                }
                None => {
                    ui.paint_rect_filled(
                        header_origin,
                        [pw, INSPECTOR_HEADER_HEIGHT],
                        theme.surface,
                        0.0,
                    );
                    // Nothing selected: teach what the panel is for rather
                    // than reporting a void.
                    let w = (pw - 32.0).max(0.0);
                    if w > 0.0 {
                        khora_tool_ui::widgets::empty_state(
                            ui,
                            &theme,
                            [px + 16.0, py + HEADER_HEIGHT + 24.0, w, 120.0],
                            Icon::Crosshair,
                            "No selection",
                            "Select an entity to inspect it.",
                        );
                    }
                    py + HEADER_HEIGHT + INSPECTOR_HEADER_HEIGHT + 96.0
                }
            }
        };

        if asset_mode {
            if let Some(rel) = asset_path.as_ref() {
                let body_y = after_header + 12.0;
                let body_h = (panel_rect[1] + panel_rect[3] - body_y - 6.0).max(0.0);
                let body_rect = [px + 6.0, body_y, pw - 12.0, body_h];
                let release = render_asset_pane(
                    ui,
                    body_rect,
                    rel,
                    &project_folder,
                    quarantine.as_ref(),
                    &theme,
                );
                if release {
                    if let Ok(mut state) = self.state.lock() {
                        state.pending_release = Some(rel.clone());
                    }
                }
            }
            return;
        }

        // Two views of the same entity: a segmented control, as tabs would
        // imply going somewhere else.
        let subtab_y = after_header + 8.0;
        let labels: Vec<&str> = self.tabs.iter().map(|t| t.label()).collect();
        if let Some(hit) = khora_tool_ui::widgets::segmented_tabs(
            ui,
            &theme,
            [px + 6.0, subtab_y, pw - 12.0, SUBTAB_HEIGHT],
            "p-sub",
            &labels,
            self.active_tab,
        ) {
            self.active_tab = hit;
        }

        let body_y = subtab_y + SUBTAB_HEIGHT + 10.0;
        let body_h = (panel_rect[1] + panel_rect[3] - body_y - 6.0).max(0.0);
        let body_rect = [px + 6.0, body_y, pw - 12.0, body_h];

        let (entity, inspected) = match entity_data {
            Some(e) => e,
            None => return,
        };

        let mut ctx = InspectorTabContext {
            state: &self.state,
            history: &self.command_history,
            theme: &theme,
            entity,
            inspected: &inspected,
        };
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.render(ui, body_rect, &mut ctx);
        }
    }
}
