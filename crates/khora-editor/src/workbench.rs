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

//! Workbench — the dockable area between the spine and the status bar.
//!
//! One [`EditorPanel`] that hosts all the others. It owns a
//! [`DockTree`] per [`EditorMode`], lays the tree out over its own rect, and
//! hands each leaf its share through [`UiBuilder::region_at`] — so the panels
//! inside see a `panel_rect()` of exactly their pane and need no notion of the
//! dock at all.
//!
//! ## Why here and not in the shell
//!
//! The dock's chrome is painted with `khora-tool-ui`, which carries Khora's
//! brand. `khora-infra` — where the egui shell lives — must not depend on it,
//! or every game built on the engine would inherit the vendor's look. Hosting
//! the dock as a Center panel keeps the branded widgets in the editor where
//! they belong, and leaves `EditorShell` generic.
//!
//! It also puts the per-mode layouts where mode knowledge already lives. The
//! shell used to reach into `EditorState::active_mode` to hide the right
//! sidebar in Control Plane mode; with a tree per mode, switching modes swaps
//! the whole layout and that special case disappears.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorMode, EditorPanel, EditorState, UiBuilder, UiTheme};
use khora_sdk::khora_core::ui::editor::dock::{zone_at, DockTree, DropZone};
use khora_tool_ui::widgets::{
    dock_drag_ghost, dock_drop_overlay, dock_splitter, dock_tab_strip, TAB_STRIP_H,
};

/// Hosts the dockable panels for every workspace.
pub struct WorkbenchPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    /// Panels by id. Owned here rather than by the shell, because the dock —
    /// not the registration order — decides where each one goes.
    panels: HashMap<String, Box<dyn EditorPanel>>,
    /// One layout per workspace. Switching modes swaps the tree wholesale.
    trees: HashMap<EditorMode, DockTree>,
    /// Panel id currently being dragged by its tab, for the whole gesture.
    ///
    /// Held as an id rather than an index: the layout is rebuilt every frame,
    /// so an index would address a different pane the moment anything moved.
    dragging: Option<String>,
    /// Where the in-flight drag would land if released now.
    pending_drop: Option<(String, DropZone)>,
}

impl WorkbenchPanel {
    /// Creates an empty workbench.
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            panels: HashMap::new(),
            trees: HashMap::new(),
            dragging: None,
            pending_drop: None,
        }
    }

    /// Adds a panel to the pool the layouts can place.
    ///
    /// A panel that no mode's tree names is simply never shown — the pool and
    /// the layouts are deliberately independent, so a mode can drop a panel
    /// without unregistering it.
    pub fn add_panel(&mut self, panel: Box<dyn EditorPanel>) {
        self.panels.insert(panel.id().to_owned(), panel);
    }

    /// Sets the layout for one workspace.
    pub fn set_layout(&mut self, mode: EditorMode, tree: DockTree) {
        self.trees.insert(mode, tree);
    }

    fn active_mode(&self) -> EditorMode {
        self.state
            .lock()
            .ok()
            .map(|s| s.active_mode)
            .unwrap_or_default()
    }
}

impl EditorPanel for WorkbenchPanel {
    fn id(&self) -> &str {
        "khora.editor.workbench"
    }

    fn title(&self) -> &str {
        "Workbench"
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let mode = self.active_mode();
        let area = ui.panel_rect();
        let theme = self.theme.clone();

        let Some(layout) = self.trees.get(&mode).map(|t| t.layout(area)) else {
            return;
        };

        let pointer = ui.pointer_position();
        let drag_active = ui.is_drag_active();

        // ── Panes ─────────────────────────────────────
        let mut activate: Option<(String, String)> = None;
        let mut drag_start: Option<String> = None;

        for (gi, group) in layout.groups.iter().enumerate() {
            let salt = format!("wb-g{gi}");
            let event = dock_tab_strip(ui, &theme, group, &salt);

            if let Some(i) = event.activated {
                if let Some(p) = group.panels.get(i) {
                    activate = Some((p.clone(), p.clone()));
                }
            }
            if let Some(i) = event.drag_started {
                if let Some(p) = group.panels.get(i) {
                    drag_start = Some(p.clone());
                }
            }

            let [gx, gy, gw, gh] = group.rect;
            let content = [gx, gy + TAB_STRIP_H, gw, (gh - TAB_STRIP_H).max(0.0)];
            if content[2] <= 1.0 || content[3] <= 1.0 {
                continue;
            }
            let Some(active_id) = group.active_panel().map(|s| s.to_owned()) else {
                continue;
            };
            if let Some(panel) = self.panels.get_mut(&active_id) {
                ui.region_at(&active_id, content, &mut |inner| panel.ui(inner));
            }
        }

        // ── Dividers ──────────────────────────────────
        for (si, splitter) in layout.splitters.iter().enumerate() {
            if let Some(ratio) = dock_splitter(ui, &theme, splitter, &format!("wb-sp{si}")) {
                if let Some(tree) = self.trees.get_mut(&mode) {
                    tree.set_ratio(splitter.id, ratio);
                }
            }
        }

        // ── Drag feedback and commit ──────────────────
        if let Some(id) = drag_start {
            self.dragging = Some(id);
        }

        if let Some(dragged) = self.dragging.clone() {
            if drag_active {
                // Recompute the target every frame from the live layout: the
                // pane under the cursor is only meaningful for the frame it
                // was measured in.
                self.pending_drop = pointer.and_then(|p| {
                    layout.groups.iter().find_map(|g| {
                        let zone = zone_at(g.rect, p)?;
                        let anchor = g.active_panel()?.to_owned();
                        Some((anchor, zone))
                    })
                });
                if let Some((anchor, zone)) = &self.pending_drop {
                    if let Some(g) = layout
                        .groups
                        .iter()
                        .find(|g| g.active_panel() == Some(anchor.as_str()))
                    {
                        dock_drop_overlay(ui, &theme, g.rect, *zone);
                    }
                }
                if let Some(p) = pointer {
                    dock_drag_ghost(ui, &theme, &dragged, p);
                }
            } else {
                // The pointer came up: commit wherever the last frame pointed.
                if let Some((anchor, zone)) = self.pending_drop.take() {
                    if anchor != dragged {
                        if let Some(tree) = self.trees.get_mut(&mode) {
                            tree.insert(dragged.clone(), Some(&anchor), zone);
                        }
                    }
                }
                self.dragging = None;
                self.pending_drop = None;
            }
        }

        if let Some((panel, _)) = activate {
            if let Some(tree) = self.trees.get_mut(&mode) {
                tree.activate(&panel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::khora_core::ui::editor::dock::DropZone;

    /// A panel the pool holds but no layout names must simply not be shown —
    /// the two are independent so a mode can drop a panel without the app
    /// having to unregister it.
    #[test]
    fn layout_selects_from_the_pool_rather_than_the_pool_driving_the_layout() {
        let state = Arc::new(Mutex::new(EditorState::default()));
        let mut wb = WorkbenchPanel::new(state, khora_tool_ui::khora_dark());

        let mut tree = DockTree::single("khora.editor.viewport");
        tree.insert("khora.editor.console", Some("khora.editor.viewport"), DropZone::Bottom);
        wb.set_layout(EditorMode::Scene, tree);

        let shown = wb.trees[&EditorMode::Scene].panels();
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().any(|p| p == "khora.editor.console"));
        assert!(
            !shown.iter().any(|p| p == "khora.editor.control_plane"),
            "a panel no tree names stays hidden"
        );
    }

    /// Each workspace keeps its own layout, so switching modes swaps the whole
    /// arrangement instead of hiding panels one by one.
    #[test]
    fn each_mode_owns_its_layout() {
        let state = Arc::new(Mutex::new(EditorState::default()));
        let mut wb = WorkbenchPanel::new(state, khora_tool_ui::khora_dark());
        wb.set_layout(EditorMode::Scene, DockTree::single("khora.editor.viewport"));
        wb.set_layout(
            EditorMode::ControlPlane,
            DockTree::single("khora.editor.control_plane"),
        );

        assert_eq!(
            wb.trees[&EditorMode::Scene].panels(),
            vec!["khora.editor.viewport"]
        );
        assert_eq!(
            wb.trees[&EditorMode::ControlPlane].panels(),
            vec!["khora.editor.control_plane"]
        );
    }
}
