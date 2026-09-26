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

//! The folder sidebar and the breadcrumb.

use std::collections::BTreeMap;

use khora_sdk::editor_ui::*;

use crate::widgets::paint::{paint_icon, paint_text_size, with_alpha};

use super::drag::unpack_asset_drag;
use super::{AssetBrowserPanel, SIDEBAR_ROW_H, TREE_INDENT};

/// A folder-tree context-menu / drag-drop action, collected during the
/// (`&self`) tree walk and applied afterwards where `&mut self` is available.
enum FolderAction {
    /// Create a "New Folder" child under this folder (`""` = assets root).
    NewFolder(String),
    /// Send this folder to the OS recycle bin.
    Delete(String),
    /// Open this folder in the OS file explorer.
    Reveal(String),
    /// Begin inline-renaming this folder, seeding the buffer with its name.
    StartRename { path: String, name: String },
    /// Move the asset at `flat` index `idx` into this folder.
    Move { idx: usize, dest: String },
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

impl AssetBrowserPanel {
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
        // Union in every real directory (including empty and freshly-created
        // ones the file-only VFS can't enumerate). These are pure structure —
        // no assets to count — so we only create the missing nodes.
        for dir in &self.asset_dirs {
            if dir.is_empty() {
                continue;
            }
            let mut cursor = &mut root;
            let mut accumulated = String::new();
            for segment in dir.split('/') {
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
            }
        }
        root
    }

    /// Render the recursive folder tree in the sidebar's lower panel.
    /// Returns the y coordinate after the last rendered row.
    pub(super) fn render_folder_tree(
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
        let mut actions: Vec<FolderAction> = Vec::new();
        // Window-space rect of the folder row being inline-renamed (if any),
        // captured during the walk so we can overlay a text field afterwards
        // where `&mut self` (hence the shared edit buffer) is available.
        let mut rename_rect: Option<[f32; 4]> = None;
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
            &mut actions,
            &mut rename_rect,
        );
        for (path, open) in new_expanded {
            self.expanded_folders.insert(path, open);
        }
        if new_current != self.current_folder {
            self.current_folder = new_current;
        }

        // Apply folder context-menu / drop actions collected during the walk.
        for action in actions {
            match action {
                FolderAction::NewFolder(parent) => self.queue_new_folder(&parent),
                FolderAction::Delete(rel) => {
                    self.confirm_delete = Some((rel, true));
                }
                FolderAction::Reveal(rel) => self.reveal_in_explorer(&rel, true),
                FolderAction::StartRename { path, name } => {
                    self.renaming_folder = Some(path);
                    self.folder_rename_buffer = name;
                    self.rename_focus_pending = true;
                }
                FolderAction::Move { idx, dest } => {
                    if let Some(src) = self.flat.get(idx).map(|a| a.rel_path.clone()) {
                        if let Ok(mut state) = self.state.lock() {
                            state.pending_move_asset = Some((src.clone(), dest.clone()));
                        }
                        log::info!("Asset browser: moving '{src}' into '{dest}'");
                    }
                }
            }
        }

        // Inline folder-rename overlay, drawn over the captured row rect.
        if let Some(rect) = rename_rect {
            let focus = self.rename_focus_pending;
            let buf = &mut self.folder_rename_buffer;
            let event = ui.inline_text_field(rect, "folder-rename", buf, focus);
            self.rename_focus_pending = false;
            match event {
                InlineEditEvent::Committed => {
                    if let Some(old) = self.renaming_folder.clone() {
                        let new_name = self.folder_rename_buffer.trim().to_string();
                        if !new_name.is_empty() {
                            let parent = old
                                .rfind('/')
                                .map(|p| old[..p].to_string())
                                .unwrap_or_default();
                            let new_rel = if parent.is_empty() {
                                new_name
                            } else {
                                format!("{parent}/{new_name}")
                            };
                            if new_rel != old {
                                if let Ok(mut state) = self.state.lock() {
                                    state.pending_rename_asset = Some((old, new_rel));
                                }
                            }
                        }
                    }
                    self.renaming_folder = None;
                }
                InlineEditEvent::Cancelled => self.renaming_folder = None,
                _ => {}
            }
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
        actions: &mut Vec<FolderAction>,
        rename_rect: &mut Option<[f32; 4]>,
    ) {
        let row_x = origin_x + 4.0;
        let row_w = sidebar_w - 8.0;
        let label = if node.name.is_empty() {
            "assets"
        } else {
            node.name.as_str()
        };
        let is_root = node.full_path.is_empty();
        let is_renaming = self.renaming_folder.as_deref() == Some(node.full_path.as_str());
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

        // Context menu (attached to this row's response) and folder drop target
        // (an asset dragged onto a folder is a move). Both run before any child
        // row registers its own `interact_rect`, so `last_response` still points
        // at this row.
        {
            let full = node.full_path.clone();
            let display = label.to_string();
            let allow_edit = !is_root;
            ui.context_menu_last(&mut |menu| {
                if menu.button("New Folder") {
                    actions.push(FolderAction::NewFolder(full.clone()));
                    menu.close_menu();
                }
                if allow_edit && menu.button("Rename") {
                    actions.push(FolderAction::StartRename {
                        path: full.clone(),
                        name: display.clone(),
                    });
                    menu.close_menu();
                }
                if allow_edit && menu.button("Delete") {
                    actions.push(FolderAction::Delete(full.clone()));
                    menu.close_menu();
                }
                menu.separator();
                if menu.button("Reveal in Explorer") {
                    actions.push(FolderAction::Reveal(full.clone()));
                    menu.close_menu();
                }
            });
        }
        if let Some(payload) = ui.dnd_take_drop_payload() {
            if let Some(idx) = unpack_asset_drag(payload, self.last_epoch.unwrap_or(0)) {
                actions.push(FolderAction::Move {
                    idx: idx as usize,
                    dest: node.full_path.clone(),
                });
            }
        }

        // A droppable target: while an asset is being dragged and this row is
        // hovered, show an accent fill + border so it's obvious a drop here
        // moves the file into this folder.
        let drop_hover = ui.is_drag_active() && interaction.hovered;
        if drop_hover {
            ui.paint_rect_filled(
                [row_x, *y],
                [row_w, SIDEBAR_ROW_H],
                with_alpha(theme.accent_a, 0.28),
                theme.radius_sm,
            );
            ui.paint_rect_stroke(
                [row_x, *y],
                [row_w, SIDEBAR_ROW_H],
                with_alpha(theme.accent_a, 0.9),
                theme.radius_sm,
                1.0,
            );
        } else if is_current {
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
        if is_renaming {
            // Leave the label blank and hand its rect to the caller, which
            // overlays a text field there once `&mut self` is available.
            *rename_rect = Some([
                icon_x + 12.0,
                *y + 2.0,
                row_w - (icon_x - row_x) - 24.0,
                18.0,
            ]);
        } else {
            paint_text_size(
                ui,
                [icon_x + 14.0, *y + 5.0],
                label,
                11.5,
                if is_current {
                    theme.text
                } else {
                    theme.text_dim
                },
            );
            ui.paint_text_styled(
                [row_x + row_w - 6.0, *y + 5.0],
                &format!("{}", node.asset_count),
                10.0,
                theme.text_muted,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
        }
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
                    actions,
                    rename_rect,
                );
            }
        }
    }

    /// Render the navigable breadcrumb (`project › assets › subfolder ›`).
    /// Each segment is clickable — sets `current_folder`.
    pub(super) fn render_breadcrumb(
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
                ui.paint_rect_filled(
                    [x, y],
                    [label_w, 18.0],
                    theme.surface_active,
                    theme.radius_sm,
                );
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
                khora_tool_ui::widgets::paint::icon(
                    ui,
                    [x, y + 2.0],
                    Icon::ChevronRight,
                    12.0,
                    theme.text_disabled,
                );
                x += 14.0;
            }
        }
    }
}
