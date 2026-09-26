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

//! Asset Browser — folder explorer + grid + per-asset inspection.
//!
//! Layout:
//!
//! ```text
//! ┌──────────────┬───────────────────────────────────┐
//! │  Categories  │  ← / breadcrumb / search →        │
//! │  (filters)   ├───────────────────────────────────┤
//! ├──────────────┤  Tile grid                        │
//! │  Folder tree │                                   │
//! │  (real VFS)  │                                   │
//! └──────────────┴───────────────────────────────────┘
//! ```
//!
//! Single-click on a tile sets `EditorState::inspected_asset_path` so
//! the Inspector switches to asset-metadata mode (Phase 5). Double-click
//! still routes through the [`handlers::AssetTypeHandler`] activation
//! — `LoadScene` for `.kscene`, `OpenExternal` for everything else.

pub mod handlers;

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::paint::with_alpha;
use crate::widgets::tile::AssetTileKind;

use handlers::{handler_for, tile_kind_for, ActivationKind};
use naming::{entity_display_name, sanitize_for_filename};

mod drag;
mod folder_tree;
mod naming;
mod panel;

pub(crate) use drag::{is_asset_drag, unpack_asset_drag};

const HEADER_HEIGHT: f32 = 34.0;

const TOOLBAR_HEIGHT: f32 = 32.0;

const SIDEBAR_WIDTH: f32 = 220.0;

const TILE_SIZE: f32 = 96.0;

const TILE_GAP: f32 = 10.0;

const SIDEBAR_ROW_H: f32 = 22.0;

const TREE_INDENT: f32 = 14.0;

#[derive(Debug, Clone)]
struct FlatAsset {
    /// Display name (file name component).
    name: String,
    /// Forward-slash relative path under `<project>/assets/`.
    rel_path: String,
    /// Folder containing the asset (forward-slash, no trailing /). `""`
    /// for assets that sit directly under `assets/`.
    folder: String,
    asset_type: AssetTileKind,
    type_name: String,
}

pub struct AssetBrowserPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    search_filter: String,
    flat: Vec<FlatAsset>,
    /// Every real directory under `assets/` (forward-slash, relative), mirrored
    /// from `EditorState::asset_dirs` so empty / freshly-created folders show up
    /// in the tree even though the file-only VFS never lists them.
    asset_dirs: Vec<String>,
    /// Last `EditorState::asset_epoch` we rebuilt `flat` for; a mismatch drives
    /// the rescan (an in-place edit that doesn't change the entry count used to
    /// be missed by the old `(folder, len)` key).
    last_epoch: Option<u64>,
    selected_filter: Option<AssetTileKind>,
    selected_index: Option<usize>,
    /// `flat` index of the tile currently being inline-renamed, if any.
    renaming_index: Option<usize>,
    /// Edit buffer backing both the tile and folder inline-rename fields.
    rename_buffer: String,
    /// Forward-slash path of the folder currently being inline-renamed, if any.
    renaming_folder: Option<String>,
    /// Edit buffer for the folder inline-rename field.
    folder_rename_buffer: String,
    /// `true` on the frame a rename starts, so the inline field grabs keyboard
    /// focus once (not every frame, which would trap focus).
    rename_focus_pending: bool,
    /// Forward-slash folder path (relative to `assets/`) currently
    /// selected in the tree. `""` = root, `None` initially.
    current_folder: Option<String>,
    /// Per-folder expand/collapse state. Keys are full paths; missing =
    /// collapsed (root is special-cased to start expanded).
    expanded_folders: std::collections::HashMap<String, bool>,
    /// Path awaiting the delete confirmation, and whether it names a folder.
    ///
    /// Every delete route parks its target here instead of writing
    /// `EditorState::pending_delete_asset` directly, so the recycle-bin call
    /// only happens once the user has answered the dialog.
    confirm_delete: Option<(String, bool)>,
    /// How far the tile grid is scrolled.
    grid_scroll: khora_tool_ui::widgets::ScrollState,
}

impl AssetBrowserPanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            search_filter: String::new(),
            flat: Vec::new(),
            asset_dirs: Vec::new(),
            last_epoch: None,
            selected_filter: None,
            selected_index: None,
            renaming_index: None,
            rename_buffer: String::new(),
            renaming_folder: None,
            folder_rename_buffer: String::new(),
            rename_focus_pending: false,
            current_folder: None,
            expanded_folders: std::collections::HashMap::new(),
            confirm_delete: None,
            grid_scroll: khora_tool_ui::widgets::ScrollState::default(),
        }
    }

    /// Asks before sending anything to the recycle bin, and only then queues
    /// the deletion for `commands::process_pending_asset_file_ops`.
    ///
    /// Deleting a file is the one action here the editor cannot undo — there is
    /// no undo history, and the file leaves the project. A folder takes
    /// everything under it, so its wording says so.
    fn render_delete_confirmation(
        &mut self,
        ui: &mut dyn UiBuilder,
        panel_rect: [f32; 4],
        theme: &UiTheme,
    ) {
        let Some((rel, is_folder)) = self.confirm_delete.clone() else {
            return;
        };
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_owned();
        let title = if is_folder {
            format!("Delete folder “{name}”?")
        } else {
            format!("Delete “{name}”?")
        };
        let body = if is_folder {
            "The folder and everything inside it go to the recycle bin."
        } else {
            "The file goes to the recycle bin."
        };

        match khora_tool_ui::widgets::confirm_modal(
            ui,
            theme,
            panel_rect,
            "ab-delete",
            khora_tool_ui::widgets::Confirm::danger(&title, body, "Delete"),
        ) {
            khora_tool_ui::widgets::ModalChoice::Confirmed => {
                if let Ok(mut state) = self.state.lock() {
                    state.pending_delete_asset = Some(rel.clone());
                }
                log::info!("Asset browser: deleting '{rel}'");
                self.confirm_delete = None;
            }
            khora_tool_ui::widgets::ModalChoice::Cancelled => {
                self.confirm_delete = None;
            }
            khora_tool_ui::widgets::ModalChoice::Pending => {}
        }
    }

    fn rescan_if_needed(&mut self) {
        let (project_folder, epoch, entries, dirs) = match self.state.lock() {
            Ok(s) => (
                s.project_folder.clone(),
                s.asset_epoch,
                s.asset_entries
                    .iter()
                    .map(|e| (e.name.clone(), e.asset_type.clone(), e.source_path.clone()))
                    .collect::<Vec<_>>(),
                s.asset_dirs.clone(),
            ),
            Err(_) => return,
        };

        if project_folder.is_none() {
            if self.last_epoch.is_some() || !self.flat.is_empty() {
                self.flat.clear();
                self.asset_dirs.clear();
                self.last_epoch = None;
                self.selected_index = None;
                self.renaming_index = None;
                self.renaming_folder = None;
                self.current_folder = None;
            }
            return;
        }

        // Rescan only when the shared epoch advances — cheaper than diffing and
        // it also catches in-place file edits that leave the entry count fixed.
        if self.last_epoch == Some(epoch) {
            return;
        }
        self.last_epoch = Some(epoch);
        self.asset_dirs = dirs;

        if let Some(idx) = self.selected_index {
            if idx >= entries.len() {
                self.selected_index = None;
            }
        }
        // A rebuild reshuffles indices — abandon any in-flight tile rename.
        self.renaming_index = None;

        self.flat = entries
            .into_iter()
            .map(|(name, asset_type_name, rel_path)| {
                let folder = rel_path
                    .rfind('/')
                    .map(|p| rel_path[..p].to_string())
                    .unwrap_or_default();
                FlatAsset {
                    name,
                    rel_path,
                    folder,
                    asset_type: tile_kind_for(&asset_type_name),
                    type_name: asset_type_name,
                }
            })
            .collect();

        if self.current_folder.is_none() {
            self.current_folder = Some(String::new());
            self.expanded_folders.insert(String::new(), true);
        }
    }

    /// Resolves a forward-slash path under `assets/` to an absolute OS path,
    /// or `None` when no project is open.
    fn absolute_rel_path(&self, rel: &str) -> Option<String> {
        let project_folder = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone())?;
        let abs = std::path::Path::new(&project_folder)
            .join("assets")
            .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        Some(abs.to_string_lossy().to_string())
    }

    fn absolute_path_for(&self, asset: &FlatAsset) -> Option<String> {
        self.absolute_rel_path(&asset.rel_path)
    }

    /// Opens the OS file explorer at `rel`. For a file we reveal its containing
    /// folder; for a directory we open the directory itself.
    fn reveal_in_explorer(&self, rel: &str, is_dir: bool) {
        let Some(abs) = self.absolute_rel_path(rel) else {
            log::warn!("Asset browser: cannot reveal '{rel}' — no project folder set");
            return;
        };
        let target = if is_dir {
            std::path::PathBuf::from(&abs)
        } else {
            std::path::Path::new(&abs)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from(&abs))
        };
        if let Err(e) = open::that(&target) {
            log::warn!("Asset browser: failed to reveal '{rel}': {e}");
        }
    }

    /// Opens the project root in the OS file explorer.
    fn reveal_project(&self) {
        match self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone())
        {
            Some(folder) => {
                if let Err(e) = open::that(&folder) {
                    log::warn!("Asset browser: failed to reveal project folder: {e}");
                }
            }
            None => log::info!("Asset browser: no project open to reveal"),
        }
    }

    /// Queues creation of a uniquely-named "New Folder" under `parent`
    /// (`""` = assets root), disambiguating against known directories.
    fn queue_new_folder(&self, parent: &str) {
        let candidate = |name: &str| {
            if parent.is_empty() {
                name.to_string()
            } else {
                format!("{parent}/{name}")
            }
        };
        let mut name = "New Folder".to_string();
        let mut n = 1;
        while self.asset_dirs.iter().any(|d| d == &candidate(&name)) {
            n += 1;
            name = format!("New Folder {n}");
        }
        let rel = candidate(&name);
        if let Ok(mut state) = self.state.lock() {
            state.pending_create_folder = Some(rel.clone());
        }
        log::info!("Asset browser: creating folder '{rel}'");
    }

    /// Paints a small chip that follows the cursor while a tile is being
    /// dragged, so it's obvious the user is carrying an asset. Drawn on top of
    /// the grid, offset from the pointer so the cursor stays visible.
    fn paint_drag_ghost(
        &self,
        ui: &mut dyn UiBuilder,
        cursor: [f32; 2],
        name: &str,
        kind: AssetTileKind,
        theme: &UiTheme,
    ) {
        let text_w = ui.measure_text(name, 11.5, FontFamilyHint::Proportional)[0];
        let pad = 8.0;
        let icon_w = 16.0;
        let w = (icon_w + text_w + pad * 2.0 + 4.0).min(220.0);
        let h = 24.0;
        let x = cursor[0] + 14.0;
        let y = cursor[1] + 6.0;
        let accent = kind.accent(theme);
        // Painted in the unclipped top overlay layer so the ghost stays visible
        // once the cursor leaves the asset-browser panel (dragging toward the
        // folder tree or the 3D viewport). Soft shadow, body, accent border.
        ui.overlay_rect_filled(
            [x + 1.0, y + 2.0],
            [w, h],
            with_alpha(theme.background, 0.45),
            theme.radius_md,
        );
        ui.overlay_rect_filled(
            [x, y],
            [w, h],
            with_alpha(theme.surface, 0.98),
            theme.radius_md,
        );
        ui.overlay_rect_stroke(
            [x, y],
            [w, h],
            with_alpha(accent, 0.9),
            theme.radius_md,
            1.0,
        );
        ui.overlay_text(
            [x + pad, y + 6.0],
            kind.icon().glyph(),
            12.0,
            accent,
            FontFamilyHint::Icons,
        );
        ui.overlay_text(
            [x + pad + icon_w, y + 6.0],
            name,
            11.5,
            theme.text,
            FontFamilyHint::Proportional,
        );
    }

    /// Queues saving `entity`'s subtree as a `.kprefab` in the current folder,
    /// using the entity's display name as the file stem. Shared by the
    /// grid-wide drop target and the per-tile entity-drop handler.
    fn queue_save_entity_as_prefab(&self, entity: khora_sdk::prelude::ecs::EntityId) {
        let folder = self.current_folder.clone().unwrap_or_default();
        if let Ok(mut state) = self.state.lock() {
            let name = entity_display_name(&state.scene_roots, entity)
                .unwrap_or_else(|| format!("Entity_{}", entity.index));
            let stem = sanitize_for_filename(&name);
            let rel_path = if folder.is_empty() {
                format!("{stem}.kprefab")
            } else {
                format!("{folder}/{stem}.kprefab")
            };
            state.pending_save_as_prefab_at = Some((entity, rel_path));
            log::info!("Asset browser: entity '{name}' dropped — creating prefab in '{folder}'");
        }
    }

    fn activate_asset(&self, asset: &FlatAsset) {
        let Some(abs_path) = self.absolute_path_for(asset) else {
            log::warn!(
                "Asset browser: cannot activate '{}' — no project folder set",
                asset.rel_path
            );
            return;
        };
        let activation = handler_for(&asset.type_name)
            .map(|h| h.activate(abs_path.clone()))
            .unwrap_or(ActivationKind::OpenExternal { abs_path });

        match activation {
            ActivationKind::LoadScene { abs_path } => {
                if let Ok(mut state) = self.state.lock() {
                    state.pending_scene_load = Some(abs_path);
                    log::info!("Asset browser: loading scene '{}'", asset.rel_path);
                }
            }
            ActivationKind::SpawnPrefab { abs_path: _ } => {
                if let Ok(mut state) = self.state.lock() {
                    state.pending_prefab_spawn = Some((asset.rel_path.clone(), None));
                    log::info!("Asset browser: spawning prefab '{}'", asset.rel_path);
                }
            }
            ActivationKind::OpenExternal { abs_path } => match open::that(&abs_path) {
                Ok(()) => log::info!(
                    "Asset browser: opened '{}' in OS-default application",
                    asset.rel_path
                ),
                Err(e) => log::warn!(
                    "Asset browser: failed to open '{}' externally: {}",
                    asset.rel_path,
                    e
                ),
            },
        }
    }
}
