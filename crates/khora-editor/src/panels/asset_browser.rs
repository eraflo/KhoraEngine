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
//! Phase G: replaces the tree-of-rows layout with the mockup's tile grid.
//! Each asset is rendered as a square thumbnail with a type-coloured gradient
//! plus a Lucide glyph. Folder navigation lives in a left column with
//! tab-like rows showing per-type counts.

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
    name: String,
    path: std::path::PathBuf,
    asset_type: AssetTileKind,
}

pub struct AssetBrowserPanel {
    state: Arc<Mutex<EditorState>>,
    theme: EditorTheme,
    search_filter: String,
    flat: Vec<FlatAsset>,
    last_scanned_folder: Option<String>,
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
            last_scanned_folder: None,
            selected_filter: None,
            selected_index: None,
        }
    }

    fn classify_extension(path: &std::path::Path) -> (AssetTileKind, &'static str) {
        // Only types the engine actually loads today are recognised.
        // `Material`, `Script` and `Prefab` were removed — clicking those
        // tiles was a no-op (no loader behind them).
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "gltf" | "glb" | "obj" | "fbx" => (AssetTileKind::Mesh, "Mesh"),
            "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" => (AssetTileKind::Texture, "Texture"),
            "wav" | "ogg" | "mp3" | "flac" => (AssetTileKind::Audio, "Audio"),
            "wgsl" | "hlsl" | "glsl" => (AssetTileKind::Shader, "Shader"),
            "kscene" | "scene" => (AssetTileKind::Scene, "Scene"),
            _ => (AssetTileKind::Unknown, "Other"),
        }
    }

    fn rescan_if_needed(&mut self) {
        let project_folder = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone());

        let folder = match project_folder {
            Some(f) => f,
            None => {
                self.flat.clear();
                self.last_scanned_folder = None;
                return;
            }
        };

        if self.last_scanned_folder.as_ref() == Some(&folder) {
            return;
        }
        self.last_scanned_folder = Some(folder.clone());
        self.flat.clear();

        let assets_dir = std::path::Path::new(&folder).join("assets");
        let scan_root = if assets_dir.is_dir() {
            assets_dir
        } else {
            std::path::PathBuf::from(&folder)
        };
        walk_dir(&scan_root, &mut self.flat);
        log::info!(
            "Asset browser: scanned '{}' — {} files",
            scan_root.display(),
            self.flat.len()
        );
    }
}

fn walk_dir(dir: &std::path::Path, out: &mut Vec<FlatAsset>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out);
        } else {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let (kind, _type_str) = AssetBrowserPanel::classify_extension(&path);
            out.push(FlatAsset {
                name,
                path: path.clone(),
                asset_type: kind,
            });
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
            // Trigger scene load on selection of a scene.
            if let Some(asset) = self.flat.get(i) {
                if asset.asset_type == AssetTileKind::Scene {
                    if let Ok(mut state) = self.state.lock() {
                        state.pending_scene_load = Some(asset.path.to_string_lossy().to_string());
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
