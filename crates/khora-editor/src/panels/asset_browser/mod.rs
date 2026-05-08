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

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::chrome::{paint_panel_header, panel_tab};
use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};
use crate::widgets::tile::{paint_asset_tile, AssetTileKind};

use handlers::{handler_for, tile_kind_for, ActivationKind};

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

/// One node of the folder tree built from the flat asset list.
#[derive(Debug, Default)]
struct FolderNode {
    /// Forward-slash full path (relative to `assets/`). `""` for the root.
    full_path: String,
    /// Last segment (display name). `""` for the root.
    name: String,
    /// Direct children, keyed by name (sorted by `BTreeMap`).
    children: BTreeMap<String, FolderNode>,
    /// Number of assets reachable from this folder (recursive total).
    asset_count: usize,
}

pub struct AssetBrowserPanel {
    state: Arc<Mutex<EditorState>>,
    theme: UiTheme,
    search_filter: String,
    flat: Vec<FlatAsset>,
    last_snapshot_key: Option<(String, usize)>,
    selected_filter: Option<AssetTileKind>,
    selected_index: Option<usize>,
    /// Forward-slash folder path (relative to `assets/`) currently
    /// selected in the tree. `""` = root, `None` initially.
    current_folder: Option<String>,
    /// Per-folder expand/collapse state. Keys are full paths; missing =
    /// collapsed (root is special-cased to start expanded).
    expanded_folders: std::collections::HashMap<String, bool>,
}

impl AssetBrowserPanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: UiTheme) -> Self {
        Self {
            state,
            theme,
            search_filter: String::new(),
            flat: Vec::new(),
            last_snapshot_key: None,
            selected_filter: None,
            selected_index: None,
            current_folder: None,
            expanded_folders: std::collections::HashMap::new(),
        }
    }

    fn rescan_if_needed(&mut self) {
        let (project_folder, entries) = match self.state.lock() {
            Ok(s) => (
                s.project_folder.clone(),
                s.asset_entries
                    .iter()
                    .map(|e| (e.name.clone(), e.asset_type.clone(), e.source_path.clone()))
                    .collect::<Vec<_>>(),
            ),
            Err(_) => return,
        };

        let folder = match project_folder {
            Some(f) => f,
            None => {
                if !self.flat.is_empty() {
                    self.flat.clear();
                    self.last_snapshot_key = None;
                    self.selected_index = None;
                    self.current_folder = None;
                }
                return;
            }
        };

        let key = (folder, entries.len());
        if self.last_snapshot_key.as_ref() == Some(&key) {
            return;
        }
        self.last_snapshot_key = Some(key);

        if let Some(idx) = self.selected_index {
            if idx >= entries.len() {
                self.selected_index = None;
            }
        }

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

    fn build_folder_tree(&self) -> FolderNode {
        let mut root = FolderNode {
            full_path: String::new(),
            name: String::new(),
            children: BTreeMap::new(),
            asset_count: 0,
        };
        for asset in &self.flat {
            root.asset_count += 1;
            if asset.folder.is_empty() {
                continue;
            }
            let mut cursor = &mut root;
            let mut accumulated = String::new();
            for segment in asset.folder.split('/') {
                if !accumulated.is_empty() {
                    accumulated.push('/');
                }
                accumulated.push_str(segment);
                cursor = cursor
                    .children
                    .entry(segment.to_string())
                    .or_insert_with(|| FolderNode {
                        full_path: accumulated.clone(),
                        name: segment.to_string(),
                        children: BTreeMap::new(),
                        asset_count: 0,
                    });
                cursor.asset_count += 1;
            }
        }
        root
    }

    fn absolute_path_for(&self, asset: &FlatAsset) -> Option<String> {
        let project_folder = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone())?;
        let abs = std::path::Path::new(&project_folder)
            .join("assets")
            .join(asset.rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        Some(abs.to_string_lossy().to_string())
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

    /// Render the recursive folder tree in the sidebar's lower panel.
    /// Returns the y coordinate after the last rendered row.
    fn render_folder_tree(
        &mut self,
        ui: &mut dyn UiBuilder,
        origin_x: f32,
        origin_y: f32,
        sidebar_w: f32,
        theme: &UiTheme,
    ) -> f32 {
        let root = self.build_folder_tree();
        let mut y = origin_y;
        let mut new_current = self.current_folder.clone();
        let mut new_expanded: Vec<(String, bool)> = Vec::new();
        self.render_folder_node(
            ui,
            &root,
            0,
            origin_x,
            &mut y,
            sidebar_w,
            theme,
            &mut new_current,
            &mut new_expanded,
        );
        for (path, open) in new_expanded {
            self.expanded_folders.insert(path, open);
        }
        if new_current != self.current_folder {
            self.current_folder = new_current;
        }
        y
    }

    #[allow(clippy::too_many_arguments)]
    fn render_folder_node(
        &self,
        ui: &mut dyn UiBuilder,
        node: &FolderNode,
        depth: u32,
        origin_x: f32,
        y: &mut f32,
        sidebar_w: f32,
        theme: &UiTheme,
        current: &mut Option<String>,
        new_expanded: &mut Vec<(String, bool)>,
    ) {
        let row_x = origin_x + 4.0;
        let row_w = sidebar_w - 8.0;
        let label = if node.name.is_empty() {
            "assets"
        } else {
            node.name.as_str()
        };
        let is_root = node.full_path.is_empty();
        let expanded = is_root
            || self
                .expanded_folders
                .get(&node.full_path)
                .copied()
                .unwrap_or(false);
        let is_current = current.as_deref() == Some(node.full_path.as_str());
        let interaction = ui.interact_rect(
            &format!("ab-tree-{}", node.full_path),
            [row_x, *y, row_w, SIDEBAR_ROW_H],
        );
        if is_current {
            ui.paint_rect_filled(
                [row_x, *y],
                [row_w, SIDEBAR_ROW_H],
                with_alpha(theme.primary, 0.14),
                theme.radius_sm,
            );
        } else if interaction.hovered {
            ui.paint_rect_filled(
                [row_x, *y],
                [row_w, SIDEBAR_ROW_H],
                with_alpha(theme.surface_active, 0.5),
                theme.radius_sm,
            );
        }
        let chev_x = row_x + 4.0 + depth as f32 * TREE_INDENT;
        let has_children = !node.children.is_empty();
        if has_children {
            let chev = if expanded {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            paint_icon(ui, [chev_x, *y + 5.0], chev, 11.0, theme.text_muted);
        }
        let icon_x = chev_x + 14.0;
        paint_icon(
            ui,
            [icon_x, *y + 5.0],
            Icon::Database,
            11.0,
            if is_current {
                theme.primary
            } else {
                theme.text_dim
            },
        );
        paint_text_size(
            ui,
            [icon_x + 14.0, *y + 5.0],
            label,
            11.5,
            if is_current { theme.text } else { theme.text_dim },
        );
        ui.paint_text_styled(
            [row_x + row_w - 6.0, *y + 5.0],
            &format!("{}", node.asset_count),
            10.0,
            theme.text_muted,
            FontFamilyHint::Monospace,
            TextAlign::Right,
        );
        if interaction.clicked {
            *current = Some(node.full_path.clone());
            // Click on a folder also flips its expand state (handy on
            // narrow sidebars where the chevron is hard to hit).
            if has_children {
                new_expanded.push((node.full_path.clone(), !expanded));
            }
        }
        *y += SIDEBAR_ROW_H + 1.0;
        if expanded {
            for child in node.children.values() {
                self.render_folder_node(
                    ui,
                    child,
                    depth + 1,
                    origin_x,
                    y,
                    sidebar_w,
                    theme,
                    current,
                    new_expanded,
                );
            }
        }
    }

    /// Render the navigable breadcrumb (`project › assets › subfolder ›`).
    /// Each segment is clickable — sets `current_folder`.
    fn render_breadcrumb(
        &mut self,
        ui: &mut dyn UiBuilder,
        origin_x: f32,
        origin_y: f32,
        max_w: f32,
        theme: &UiTheme,
        project_folder: &str,
    ) {
        let project_name = std::path::Path::new(project_folder)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        let folder = self.current_folder.clone().unwrap_or_default();
        let mut segments: Vec<(String, String)> = Vec::new();
        segments.push((project_name.to_string(), "__project".to_string()));
        segments.push(("assets".to_string(), String::new()));
        if !folder.is_empty() {
            let mut accumulated = String::new();
            for seg in folder.split('/') {
                if !accumulated.is_empty() {
                    accumulated.push('/');
                }
                accumulated.push_str(seg);
                segments.push((seg.to_string(), accumulated.clone()));
            }
        }

        let mut x = origin_x;
        let y = origin_y;
        for (i, (label, target)) in segments.iter().enumerate() {
            let active = i == segments.len() - 1;
            let label_w = ui.measure_text(label, 11.0, FontFamilyHint::Monospace)[0] + 6.0;
            if x + label_w > origin_x + max_w {
                break;
            }
            let rect = [x, y, label_w, 18.0];
            let salt = format!("ab-crumb-{}", i);
            let int = ui.interact_rect(&salt, rect);
            if int.hovered && !active {
                ui.paint_rect_filled([x, y], [label_w, 18.0], theme.surface_active, theme.radius_sm);
            }
            ui.paint_text_styled(
                [x + 3.0, y + 2.5],
                label,
                11.0,
                if active { theme.text } else { theme.text_dim },
                FontFamilyHint::Monospace,
                TextAlign::Left,
            );
            if int.clicked && target != "__project" && !active {
                self.current_folder = Some(target.clone());
            }
            x += label_w;
            if i < segments.len() - 1 {
                paint_text_size(ui, [x, y + 2.5], "›", 11.0, theme.text_muted);
                x += 10.0;
            }
        }
    }
}

const SIDEBAR_CATEGORIES: &[(AssetTileKind, &str, Icon)] = &[
    (AssetTileKind::Unknown, "All Assets", Icon::Database),
    (AssetTileKind::Scene, "Scenes", Icon::Film),
    (AssetTileKind::Mesh, "Meshes", Icon::Cube),
    (AssetTileKind::Texture, "Textures", Icon::Image),
    (AssetTileKind::Audio, "Audio", Icon::Music),
    (AssetTileKind::Shader, "Shaders", Icon::Zap),
    (AssetTileKind::Script, "Scripts", Icon::Code),
];

impl EditorPanel for AssetBrowserPanel {
    fn id(&self) -> &str {
        "asset_browser"
    }
    fn title(&self) -> &str {
        "Assets"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        self.rescan_if_needed();

        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, ph] = panel_rect;

        // ── Header ────────────────────────────────────
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, &theme);
        let tab_y = py + (HEADER_HEIGHT - 22.0) * 0.5;
        let badge = format!("{}", self.flat.len());
        let _ = panel_tab(
            ui,
            "ab-tab-assets",
            [px + 6.0, tab_y],
            "Assets",
            Some(&badge),
            true,
            &theme,
        );

        let mut ax = px + pw - 8.0;
        for (icon, salt) in [
            (Icon::More, "ab-more"),
            (Icon::Trash, "ab-trash"),
            (Icon::Filter, "ab-filter"),
        ] {
            ax -= 22.0;
            let int = ui.interact_rect(salt, [ax, py + 6.0, 22.0, 22.0]);
            if int.hovered {
                ui.paint_rect_filled([ax, py + 6.0], [22.0, 22.0], theme.surface_active, 4.0);
            }
            paint_icon(ui, [ax + 5.0, py + 11.0], icon, 13.0, theme.text_dim);
        }

        // ── Layout: sidebar | grid ────────────────────
        let body_y = py + HEADER_HEIGHT;
        let body_h = ph - HEADER_HEIGHT;
        let sidebar_w = SIDEBAR_WIDTH;

        ui.paint_rect_filled(
            [px, body_y],
            [sidebar_w, body_h],
            theme.surface_elevated,
            0.0,
        );
        ui.paint_line(
            [px + sidebar_w, body_y],
            [px + sidebar_w, body_y + body_h],
            with_alpha(theme.separator, 0.55),
            1.0,
        );

        // Type-category filter rows (compact, top of sidebar).
        let mut row_y = body_y + 8.0;
        let row_h = 22.0;
        let mut new_filter = self.selected_filter;
        for (kind, label, icon) in SIDEBAR_CATEGORIES {
            let count = if *kind == AssetTileKind::Unknown {
                self.flat.len()
            } else {
                self.flat.iter().filter(|a| a.asset_type == *kind).count()
            };
            let active = self.selected_filter == Some(*kind)
                || (self.selected_filter.is_none() && *kind == AssetTileKind::Unknown);
            let row_x = px + 4.0;
            let row_w = sidebar_w - 8.0;
            let interaction =
                ui.interact_rect(&format!("ab-side-{}", label), [row_x, row_y, row_w, row_h]);
            if active {
                ui.paint_rect_filled(
                    [row_x, row_y],
                    [row_w, row_h],
                    with_alpha(theme.primary, 0.14),
                    theme.radius_sm,
                );
            } else if interaction.hovered {
                ui.paint_rect_filled(
                    [row_x, row_y],
                    [row_w, row_h],
                    with_alpha(theme.surface_active, 0.5),
                    theme.radius_sm,
                );
            }
            paint_icon(
                ui,
                [row_x + 8.0, row_y + 5.0],
                *icon,
                12.0,
                if active {
                    theme.primary
                } else {
                    theme.text_dim
                },
            );
            paint_text_size(
                ui,
                [row_x + 26.0, row_y + 5.0],
                label,
                11.5,
                if active { theme.text } else { theme.text_dim },
            );
            ui.paint_text_styled(
                [row_x + row_w - 6.0, row_y + 5.0],
                &format!("{}", count),
                10.0,
                theme.text_muted,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
            if interaction.clicked {
                new_filter = if *kind == AssetTileKind::Unknown {
                    None
                } else {
                    Some(*kind)
                };
            }
            row_y += row_h + 1.0;
        }
        self.selected_filter = new_filter;

        // Tree separator label.
        row_y += 8.0;
        paint_text_size(
            ui,
            [px + 8.0, row_y],
            "FOLDERS",
            10.0,
            theme.text_muted,
        );
        row_y += 16.0;

        // Folder tree (real VFS hierarchy).
        let _ = self.render_folder_tree(ui, px, row_y, sidebar_w, &theme);

        // ── Grid area ─────────────────────────────────
        let grid_x = px + sidebar_w;
        let grid_w = pw - sidebar_w;

        let crumb_y = body_y;
        ui.paint_rect_filled(
            [grid_x, crumb_y],
            [grid_w, TOOLBAR_HEIGHT],
            theme.surface,
            0.0,
        );
        ui.paint_line(
            [grid_x, crumb_y + TOOLBAR_HEIGHT],
            [grid_x + grid_w, crumb_y + TOOLBAR_HEIGHT],
            with_alpha(theme.separator, 0.55),
            1.0,
        );
        let project_folder = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone())
            .unwrap_or_default();

        let search_w = 200.0_f32.min(grid_w * 0.4);
        let search_x = grid_x + grid_w - search_w - 12.0;
        let search_y = crumb_y + 4.0;

        // Breadcrumb on the left, capped to leave room for the search.
        let crumb_max_w = (search_x - grid_x - 24.0).max(0.0);
        self.render_breadcrumb(ui, grid_x + 12.0, crumb_y + 7.0, crumb_max_w, &theme, &project_folder);

        // Search affordance icon.
        paint_icon(
            ui,
            [search_x + 4.0, search_y + 5.0],
            Icon::Search,
            12.0,
            theme.text_muted,
        );

        // ── Tile grid ─────────────────────────────────
        let grid_inner_x = grid_x + 12.0;
        let grid_inner_y = crumb_y + TOOLBAR_HEIGHT + 12.0;
        let grid_inner_w = grid_w - 24.0;
        let cols = ((grid_inner_w + TILE_GAP) / (TILE_SIZE + TILE_GAP))
            .max(1.0)
            .floor() as usize;

        let filter_text = self.search_filter.to_lowercase();
        let current_folder = self.current_folder.clone().unwrap_or_default();
        let visible: Vec<(usize, &FlatAsset)> = self
            .flat
            .iter()
            .enumerate()
            .filter(|(_, a)| match self.selected_filter {
                Some(k) => a.asset_type == k,
                None => true,
            })
            .filter(|(_, a)| {
                // When a folder is selected, only show assets that live
                // *within* it (recursive — `assets/props` matches
                // `assets/props/crates/a.png`).
                if current_folder.is_empty() {
                    true
                } else {
                    a.folder == current_folder
                        || a.folder.starts_with(&format!("{}/", current_folder))
                }
            })
            .filter(|(_, a)| {
                filter_text.is_empty() || a.name.to_lowercase().contains(&filter_text)
            })
            .collect();

        let tile_h = TILE_SIZE + 22.0;
        let mut to_select: Option<usize> = None;
        let mut to_activate: Option<usize> = None;
        for (i, (orig_idx, asset)) in visible.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let tx = grid_inner_x + col as f32 * (TILE_SIZE + TILE_GAP);
            let ty = grid_inner_y + row as f32 * (tile_h + TILE_GAP);
            let selected = self.selected_index == Some(*orig_idx);
            let interaction = paint_asset_tile(
                ui,
                &format!("tile-{}", orig_idx),
                [tx, ty],
                [TILE_SIZE, tile_h],
                &asset.name,
                asset.asset_type,
                selected,
                &theme,
            );
            if interaction.double_clicked {
                to_activate = Some(*orig_idx);
            } else if interaction.clicked {
                to_select = Some(*orig_idx);
            }
        }
        if let Some(i) = to_select {
            self.selected_index = Some(i);
            if let Some(asset) = self.flat.get(i) {
                if let Ok(mut state) = self.state.lock() {
                    // Single-click switches the inspector to asset
                    // metadata mode (Phase 5). Clearing entity
                    // selection prevents the entity inspector from
                    // ghosting under the asset metadata pane.
                    state.inspected_asset_path = Some(asset.rel_path.clone());
                    state.selected_asset = Some(i);
                    state.clear_selection();
                    state.inspected = None;
                }
                log::info!(
                    "Asset selected: {} ({:?}) — {}",
                    asset.name,
                    asset.asset_type,
                    asset.rel_path
                );
            }
        }
        if let Some(i) = to_activate {
            self.selected_index = Some(i);
            if let Some(asset) = self.flat.get(i).cloned() {
                self.activate_asset(&asset);
            }
        }

        if visible.is_empty() {
            ui.paint_text_styled(
                [grid_inner_x + grid_inner_w * 0.5, grid_inner_y + 60.0],
                "No assets match the current filter.",
                12.0,
                theme.text_muted,
                FontFamilyHint::Proportional,
                TextAlign::Center,
            );
        }

        let search_filter_ref = &mut self.search_filter;
        ui.region_at(
            [search_x + 20.0, search_y, search_w - 22.0, 22.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(search_filter_ref);
            },
        );
    }
}
