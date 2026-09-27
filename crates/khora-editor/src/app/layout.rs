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

//! The dock layouts and the panels registered in them.

use khora_sdk::editor_ui::viewport_texture::ViewportTextureHandle;
use khora_sdk::Runtime;
use khora_sdk::{EditorMode, EditorShell, PanelLocation};
use khora_tool_ui::dock::{DockTree, DropZone};

use super::EditorApp;
use crate::chrome::{SpinePanel, StatusBarPanel, TitleBarPanel};
use crate::panels::command_palette::CommandPalettePanel;
use crate::panels::{
    AssetBrowserPanel, ConsolePanel, ControlPlanePanel, PropertiesPanel, SceneTreePanel,
    ViewportPanel,
};

impl EditorApp {
    /// Default Scene arrangement: hierarchy left, inspector right, viewport in
    /// the middle, assets and console sharing a dock below it.
    ///
    /// Expressed as a sequence of drops because that is exactly what the user
    /// would do by hand — which keeps the default honest about what the dock
    /// can express.
    fn scene_layout() -> DockTree {
        const VIEWPORT: &str = "khora.editor.viewport";
        let mut t = DockTree::single(VIEWPORT);

        // Each drop nests inside the previous one, so the ratios are fractions
        // of what is left rather than of the window — hence the second value
        // being much larger than the sliver of screen the inspector gets.
        let hierarchy = t.insert("khora.editor.scene_tree", Some(VIEWPORT), DropZone::Left);
        let inspector = t.insert("khora.editor.properties", Some(VIEWPORT), DropZone::Right);
        let bottom = t.insert(
            "khora.editor.asset_browser",
            Some(VIEWPORT),
            DropZone::Bottom,
        );
        t.insert(
            "khora.editor.console",
            Some("khora.editor.asset_browser"),
            DropZone::Center,
        );

        if let Some(id) = hierarchy {
            t.set_ratio(id, 0.18); // hierarchy, of the whole width
        }
        if let Some(id) = inspector {
            t.set_ratio(id, 0.74); // leaves the inspector ~21% of the window
        }
        if let Some(id) = bottom {
            t.set_ratio(id, 0.72); // viewport keeps most of the centre column
        }

        // The asset browser is the tab that opens, matching the previous dock.
        t.activate("khora.editor.asset_browser");
        t
    }

    /// Control Plane is a single full-bleed workspace. The hierarchy and the
    /// bottom dock used to stay on screen here with nothing to show, because
    /// the old shell could only hide one slot.
    fn control_plane_layout() -> DockTree {
        DockTree::single("khora.editor.control_plane")
    }

    pub(super) fn register_panels(&mut self, runtime: &Runtime) {
        let viewport_handle = runtime
            .resources
            .get::<ViewportTextureHandle>()
            .copied()
            .unwrap_or(khora_sdk::PRIMARY_VIEWPORT);

        let Some(shell_ref) = runtime
            .backends
            .get::<std::sync::Arc<std::sync::Mutex<Box<dyn EditorShell>>>>()
            .cloned()
        else {
            log::warn!("EditorApp: no EditorShell found in runtime.backends.");
            return;
        };

        if let Ok(mut shell) = shell_ref.lock() {
            shell.set_editor_state(self.editor_state.clone());

            // ── Brand identity (theme + typefaces) ─────────
            let brand_theme = khora_tool_ui::khora_dark();
            shell.set_theme(brand_theme.clone());
            shell.set_fonts(khora_tool_ui::fonts::brand_fonts());

            // ── Chrome (top, spine, status bar) ────────────
            shell.register_panel(
                PanelLocation::TopBar,
                Box::new(TitleBarPanel::new(
                    self.editor_state.clone(),
                    brand_theme.clone(),
                )),
            );
            shell.register_panel(
                PanelLocation::Spine,
                Box::new(SpinePanel::new(
                    self.editor_state.clone(),
                    brand_theme.clone(),
                )),
            );
            shell.register_panel(
                PanelLocation::StatusBar,
                Box::new(StatusBarPanel::new(
                    self.editor_state.clone(),
                    brand_theme.clone(),
                )),
            );

            // ── Workbench (the dockable area) ──────────────
            // Every working panel lives in one dock rather than a fixed slot,
            // so the user can rearrange them. The layouts below are only the
            // defaults; the dock owns the arrangement from then on.
            let mut workbench = crate::panels::workbench::WorkbenchPanel::new(
                self.editor_state.clone(),
                brand_theme.clone(),
            );
            workbench.add_panel(Box::new(SceneTreePanel::new(
                self.editor_state.clone(),
                brand_theme.clone(),
            )));
            workbench.add_panel(Box::new(PropertiesPanel::new(
                self.editor_state.clone(),
                self.command_history.clone(),
                brand_theme.clone(),
            )));
            workbench.add_panel(Box::new(AssetBrowserPanel::new(
                self.editor_state.clone(),
                brand_theme.clone(),
            )));
            workbench.add_panel(Box::new(ConsolePanel::new(
                self.editor_state.clone(),
                brand_theme.clone(),
            )));
            workbench.add_panel(Box::new(ViewportPanel::new(
                viewport_handle,
                self.editor_state.clone(),
                self.camera.clone(),
                brand_theme.clone(),
            )));
            workbench.add_panel(Box::new(ControlPlanePanel::new(
                self.editor_state.clone(),
                brand_theme.clone(),
                self.agent_registry.clone(),
                self.dcc_context.clone(),
            )));
            workbench.set_layout(EditorMode::Scene, Self::scene_layout());
            workbench.set_layout(EditorMode::ControlPlane, Self::control_plane_layout());
            shell.register_panel(PanelLocation::Center, Box::new(workbench));

            // ── Floating overlays ──────────────────────────
            shell.register_panel(
                PanelLocation::Floating(100),
                Box::new(CommandPalettePanel::new(
                    self.editor_state.clone(),
                    brand_theme.clone(),
                )),
            );
            log::info!("EditorApp: panels registered with shell.");
        }
        self.shell = Some(shell_ref);
    }
}
