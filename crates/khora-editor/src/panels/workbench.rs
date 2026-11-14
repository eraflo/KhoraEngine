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

//! The workbench: the dockable area between the spine and the status bar, one
//! panel hosting the others in a dock tree per editor mode. It lives here, not
//! in the shell, so the shell never depends on the branded `khora-tool-ui`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::{EditorMode, EditorPanel, EditorState, UiBuilder, UiTheme};
use khora_tool_ui::dock::{zone_at, DockTree, DropZone};
use khora_tool_ui::widgets::{
    dock_drag_ghost, dock_drop_overlay, dock_splitter, dock_tab_strip, TAB_STRIP_H,
};

/// Hosts the dockable panels for every workspace.
///
/// Each panel paints into its pane through [`UiBuilder::region_at`], so its
/// `panel_rect()` is exactly its pane.
pub struct WorkbenchPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    /// Panels by id: the dock, not the registration order, places them.
    panels: HashMap<String, Box<dyn EditorPanel>>,
    /// One layout per workspace. Switching modes swaps the tree wholesale.
    trees: HashMap<EditorMode, DockTree>,
    /// Panel id being dragged by its tab, for the whole gesture.
    ///
    /// An id, not an index: the layout is rebuilt every frame.
    dragging: Option<String>,
    /// Where the drag under way would land if released.
    pending_drop: Option<(String, DropZone)>,
}

impl WorkbenchPanel {
    /// Returns an empty workbench.
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
    /// A panel no mode's tree names is never shown: a mode drops a panel
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

        for (si, splitter) in layout.splitters.iter().enumerate() {
            if let Some(ratio) = dock_splitter(ui, &theme, splitter, &format!("wb-sp{si}")) {
                if let Some(tree) = self.trees.get_mut(&mode) {
                    tree.set_ratio(splitter.id, ratio);
                }
            }
        }

        if let Some(id) = drag_start {
            self.dragging = Some(id);
        }

        if let Some(dragged) = self.dragging.clone() {
            if drag_active {
                // Measured again every frame: the pane under the cursor is
                // only meaningful for the layout it was measured in.
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
    use khora_tool_ui::dock::DropZone;

    /// A panel the pool holds but no layout names is not shown, so a mode
    /// drops a panel without unregistering it.
    #[test]
    fn layout_selects_from_the_pool_rather_than_the_pool_driving_the_layout() {
        let state = Arc::new(Mutex::new(EditorState::default()));
        let mut wb = WorkbenchPanel::new(state, khora_tool_ui::khora_dark());

        let mut tree = DockTree::single("khora.editor.viewport");
        tree.insert(
            "khora.editor.console",
            Some("khora.editor.viewport"),
            DropZone::Bottom,
        );
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
