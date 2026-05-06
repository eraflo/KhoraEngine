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

//! Asset Browser — branded panel header + grid of gradient tiles per type.
//!
//! As of Phase 1 of the VFS integration, the panel **derives its asset list
//! from `EditorState::asset_entries`** instead of walking the disk itself.
//! That field is populated by `EditorApp::setup` and the hot-reload pump in
//! `EditorApp::before_agents` from `ProjectVfs::asset_service.vfs().iter_all()`,
//! so what the user sees here is always the live VFS index — no parallel
//! filesystem walk, no stale state.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;

use crate::widgets::chrome::{paint_panel_header, panel_tab};
use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};
use crate::widgets::tile::{paint_asset_tile, AssetTileKind};

const HEADER_HEIGHT: f32 = 34.0;
const TOOLBAR_HEIGHT: f32 = 32.0;
const SIDEBAR_WIDTH: f32 = 200.0;
const TILE_SIZE: f32 = 96.0;
const TILE_GAP: f32 = 10.0;

#[derive(Debug, Clone)]
struct FlatAsset {
    /// Display name (file name component).
    name: String,
    /// Forward-slash relative path under `<project>/assets/`. Stable across
    /// hot-reload and across editor / runtime — same string the VFS uses to
    /// derive `AssetUUID::new_v5`.
    rel_path: String,
    asset_type: AssetTileKind,
}

pub struct AssetBrowserPanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
    search_filter: String,
    flat: Vec<FlatAsset>,
    /// Cache invalidation key. Combines project folder + the entry-list
    /// length so reindex events from the hot-reload pump propagate as soon
    /// as `EditorState::asset_entries` is updated.
    last_snapshot_key: Option<(String, usize)>,
    selected_filter: Option<AssetTileKind>,
    selected_index: Option<usize>,
}

impl AssetBrowserPanel {
    pub fn new(state: Arc<Mutex<EditorState>>, theme: EditorTheme) -> Self {
        Self {
            state,
            theme,
            search_filter: String::new(),
            flat: Vec::new(),
            last_snapshot_key: None,
            selected_filter: None,
            selected_index: None,
        }
    }

    /// Maps the canonical lower-case `asset_type_name` produced by
    /// `IndexBuilder` (and stored in `AssetMetadata.asset_type_name`) onto
    /// the panel's tile-kind enum. Single source of truth lives in
    /// `khora_io::asset::asset_type_for_extension`; this is just the
    /// presentation-layer mapping.
    fn tile_kind_from_type_name(name: &str) -> AssetTileKind {
        match name {
            "mesh" => AssetTileKind::Mesh,
            "texture" => AssetTileKind::Texture,
            "audio" => AssetTileKind::Audio,
            "shader" => AssetTileKind::Shader,
            "scene" => AssetTileKind::Scene,
            // "font", "material", "script" and unknown types collapse onto
            // the Unknown tile for now — the tile palette only has art for
            // the five categories above.
            _ => AssetTileKind::Unknown,
        }
    }

    /// Re-pulls the asset list from `EditorState::asset_entries` whenever the
    /// snapshot key (project folder + entry count) changes. The entries are
    /// produced by the editor's hot-reload pump and `setup`, both of which
    /// read straight from the VFS.
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
                }
                return;
            }
        };

        let key = (folder, entries.len());
        if self.last_snapshot_key.as_ref() == Some(&key) {
            return;
        }
        self.last_snapshot_key = Some(key);

        // Selection survives only if the entry list shrank below it.
        if let Some(idx) = self.selected_index {
            if idx >= entries.len() {
                self.selected_index = None;
            }
        }

        self.flat = entries
            .into_iter()
            .map(|(name, asset_type_name, rel_path)| FlatAsset {
                name,
                rel_path,
                asset_type: Self::tile_kind_from_type_name(&asset_type_name),
            })
            .collect();
    }

    /// Builds the absolute on-disk path for a `FlatAsset` from
    /// `<project_folder>/assets/<rel_path>`. Used when feeding
    /// `pending_scene_load` — the dispatcher in `main.rs::update` then
    /// recognises the path as project-internal and routes through the VFS.
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
}

const SIDEBAR_CATEGORIES: &[(AssetTileKind, &str, Icon)] = &[
    (AssetTileKind::Unknown, "All Assets", Icon::Database),
    (AssetTileKind::Scene, "Scenes", Icon::Film),
    (AssetTileKind::Mesh, "Meshes", Icon::Cube),
    (AssetTileKind::Texture, "Textures", Icon::Image),
    (AssetTileKind::Audio, "Audio", Icon::Music),
    (AssetTileKind::Shader, "Shaders", Icon::Zap),
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

        // Right action icons
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

        // Sidebar background
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

        // Sidebar rows
        let mut row_y = body_y + 8.0;
        let row_h = 26.0;
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
                [row_x + 8.0, row_y + 7.0],
                *icon,
                13.0,
                if active {
                    theme.primary
                } else {
                    theme.text_dim
                },
            );
            paint_text_size(
                ui,
                [row_x + 26.0, row_y + 7.0],
                label,
                12.0,
                if active { theme.text } else { theme.text_dim },
            );
            ui.paint_text_styled(
                [row_x + row_w - 6.0, row_y + 7.0],
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

        // ── Grid area ─────────────────────────────────
        let grid_x = px + sidebar_w;
        let grid_w = pw - sidebar_w;

        // Toolbar (crumb + search)
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
        let crumb_label = match self.selected_filter {
            Some(k) => format!("{} > assets > {:?}", project_basename(&project_folder), k),
            None => format!("{} > assets", project_basename(&project_folder)),
        };
        ui.paint_text_styled(
            [grid_x + 12.0, crumb_y + 11.0],
            &crumb_label,
            11.0,
            theme.text_dim,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );

        // Search input (right side) — pure native egui widget so the visible
        // text always tracks the actual buffer. No painted pill behind, that
        // caused the "search input fantôme" bug (placeholder + typed text
        // overlapping when sizes differ).
        let search_w = 200.0_f32.min(grid_w * 0.4);
        let search_x = grid_x + grid_w - search_w - 12.0;
        let search_y = crumb_y + 4.0;
        // Just the search icon as a leading affordance — non-interactive.
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
        let visible: Vec<(usize, &FlatAsset)> = self
            .flat
            .iter()
            .enumerate()
            .filter(|(_, a)| match self.selected_filter {
                Some(k) => a.asset_type == k,
                None => true,
            })
            .filter(|(_, a)| filter_text.is_empty() || a.name.to_lowercase().contains(&filter_text))
            .collect();

        let tile_h = TILE_SIZE + 22.0;
        let mut to_select: Option<usize> = None;
        for (i, (orig_idx, asset)) in visible.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let tx = grid_inner_x + col as f32 * (TILE_SIZE + TILE_GAP);
            let ty = grid_inner_y + row as f32 * (tile_h + TILE_GAP);
            let selected = self.selected_index == Some(*orig_idx);
            if paint_asset_tile(
                ui,
                &format!("tile-{}", orig_idx),
                [tx, ty],
                [TILE_SIZE, tile_h],
                &asset.name,
                asset.asset_type,
                selected,
                &theme,
            ) {
                to_select = Some(*orig_idx);
            }
        }
        if let Some(i) = to_select {
            self.selected_index = Some(i);
            // Trigger scene load when the selected asset is a scene. Build
            // the absolute path so the editor's load dispatcher can spot
            // that the path lives under the project's assets root and route
            // through the VFS.
            if let Some(asset) = self.flat.get(i).cloned() {
                if asset.asset_type == AssetTileKind::Scene {
                    if let Some(abs) = self.absolute_path_for(&asset) {
                        if let Ok(mut state) = self.state.lock() {
                            state.pending_scene_load = Some(abs);
                        }
                    }
                }
            }
        }

        // Empty-state hint
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

        // ── Native search input ───────────────────────
        let search_filter_ref = &mut self.search_filter;
        ui.region_at(
            [search_x + 20.0, search_y, search_w - 22.0, 22.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(search_filter_ref);
            },
        );
    }
}

fn project_basename(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project")
}
