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

//! Asset Browser panel — navigable folder tree from the project's asset directory.

use std::sync::{Arc, Mutex};

use khora_core::ui::editor::*;
use khora_sdk::prelude::*;

/// A node in the asset tree (either a folder or a file).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AssetNode {
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_folder: bool,
    pub asset_type: Option<String>,
    pub children: Vec<AssetNode>,
}

impl AssetNode {
    /// Build an asset tree recursively from a filesystem directory.
    pub fn build_tree(dir: &std::path::Path) -> Option<AssetNode> {
        if !dir.is_dir() {
            return None;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "assets".to_owned());

        let mut children = Vec::new();
        let read = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => {
                return Some(AssetNode {
                    name,
                    path: dir.to_path_buf(),
                    is_folder: true,
                    asset_type: None,
                    children,
                });
            }
        };

        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                if let Some(child) = Self::build_tree(&path) {
                    children.push(child);
                }
            } else {
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let asset_type = path.extension().and_then(|e| e.to_str()).map(|ext| {
                    match ext.to_lowercase().as_str() {
                        "gltf" | "glb" | "obj" | "fbx" => "Mesh",
                        "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" => "Texture",
                        "wav" | "ogg" | "mp3" | "flac" => "Audio",
                        "wgsl" | "hlsl" | "glsl" => "Shader",
                        "ttf" | "otf" => "Font",
                        "scene" | "kscene" | "json" => "Scene",
                        "mat" | "kmat" => "Material",
                        _ => "Other",
                    }
                });
                children.push(AssetNode {
                    name: file_name,
                    path: path.clone(),
                    is_folder: false,
                    asset_type: asset_type.map(|s| s.to_owned()),
                    children: Vec::new(),
                });
            }
        }

        Some(AssetNode {
            name,
            path: dir.to_path_buf(),
            is_folder: true,
            asset_type: None,
            children,
        })
    }

    /// Count all file (non-folder) nodes recursively.
    pub fn file_count(&self) -> usize {
        if !self.is_folder {
            return 1;
        }
        self.children.iter().map(|c| c.file_count()).sum()
    }

    /// Returns true if this node or any descendant matches the filter.
    pub fn matches_filter(&self, filter: &str) -> bool {
        let lower = filter.to_lowercase();
        if self.name.to_lowercase().contains(&lower) {
            return true;
        }
        self.children.iter().any(|c| c.matches_filter(filter))
    }
}

pub struct AssetBrowserPanel {
    state: Arc<Mutex<EditorState>>,
    search_filter: String,
    asset_tree: Option<AssetNode>,
    last_scanned_folder: Option<String>,
}

impl AssetBrowserPanel {
    pub fn new(state: Arc<Mutex<EditorState>>) -> Self {
        Self {
            state,
            search_filter: String::new(),
            asset_tree: None,
            last_scanned_folder: None,
        }
    }

    fn icon_for_type(asset_type: Option<&str>) -> &'static str {
        match asset_type {
            Some("Mesh") => "\u{1F9CA}",
            Some("Texture") => "\u{1F5BC}",
            Some("Audio") => "\u{1F50A}",
            Some("Shader") => "\u{2728}",
            Some("Font") => "\u{1F524}",
            Some("Scene") => "\u{1F3AC}",
            Some("Material") => "\u{1F3A8}",
            _ => "\u{1F4C4}",
        }
    }

    fn render_tree_node(
        ui: &mut dyn UiBuilder,
        node: &AssetNode,
        filter: &str,
        pending_scene_load: &mut Option<String>,
    ) {
        if !filter.is_empty() && !node.matches_filter(filter) {
            return;
        }

        if node.is_folder {
            let header = format!("\u{1F4C1} {} ({})", node.name, node.file_count());
            ui.collapsing(&header, false, &mut |ui| {
                for child in &node.children {
                    Self::render_tree_node(ui, child, filter, pending_scene_load);
                }
            });
        } else {
            let icon = Self::icon_for_type(node.asset_type.as_deref());
            let label = format!("{} {}", icon, node.name);
            ui.horizontal(&mut |ui| {
                ui.selectable_label(false, &label);

                // Double-click on a scene file → queue load
                if ui.is_last_item_double_clicked() && node.asset_type.as_deref() == Some("Scene") {
                    *pending_scene_load = Some(node.path.to_string_lossy().to_string());
                }

                if let Some(ref ty) = node.asset_type {
                    ui.small_label(&format!("({})", ty));
                }
            });
        }
    }
}

impl EditorPanel for AssetBrowserPanel {
    fn id(&self) -> &str {
        "asset_browser"
    }
    fn title(&self) -> &str {
        "Assets"
    }
    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        // Toolbar: search only (no folder picker, no import).
        ui.horizontal(&mut |ui| {
            ui.label("\u{1F50D}");
            ui.text_edit_singleline(&mut self.search_filter);
        });

        // Show current project folder path (truncated if too long).
        let project_folder = self
            .state
            .lock()
            .ok()
            .and_then(|s| s.project_folder.clone());

        if let Some(ref folder) = project_folder {
            let display = std::path::Path::new(folder)
                .iter()
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<std::path::PathBuf>()
                .to_string_lossy()
                .to_string();
            ui.small_label(&format!("\u{1F4C2} .../{}", display));

            // Rebuild tree if folder changed.
            let needs_scan = self.last_scanned_folder.as_ref() != Some(folder);
            if needs_scan {
                let assets_dir = std::path::Path::new(folder).join("assets");
                let scan_root = if assets_dir.is_dir() {
                    &assets_dir
                } else {
                    std::path::Path::new(folder)
                };
                self.asset_tree = AssetNode::build_tree(scan_root);
                self.last_scanned_folder = Some(folder.clone());
                let count = self.asset_tree.as_ref().map_or(0, |t| t.file_count());
                log::info!(
                    "Asset browser: scanned '{}' — {} files found",
                    scan_root.display(),
                    count
                );
            }
        } else {
            ui.small_label("No project opened.");
            self.asset_tree = None;
            self.last_scanned_folder = None;
        }

        ui.separator();

        let filter = self.search_filter.clone();
        let mut pending_scene_load: Option<String> = None;

        ui.scroll_area("asset_browser_scroll", &mut |ui| match &self.asset_tree {
            Some(tree) if !tree.children.is_empty() => {
                for child in &tree.children {
                    Self::render_tree_node(ui, child, &filter, &mut pending_scene_load);
                }
            }
            Some(_) => {
                ui.colored_label([0.5, 0.5, 0.5, 1.0], "Empty folder.");
            }
            None => {
                ui.colored_label([0.5, 0.5, 0.5, 1.0], "No assets found.");
            }
        });

        // Consume double-clicked scene → queue load for update()
        if let Some(path) = pending_scene_load {
            if let Ok(mut state) = self.state.lock() {
                state.pending_scene_load = Some(path);
            }
        }
    }
}
