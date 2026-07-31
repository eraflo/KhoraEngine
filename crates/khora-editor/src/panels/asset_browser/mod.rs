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

/// High 32 bits of a `u64` drag payload that identifies an asset-browser
/// tile — the low 32 bits hold the asset index in `EditorState::asset_entries`.
/// Every tile is a drag source; the drop sink (viewport / folder tree)
/// dispatches by the asset's type, so the tag is type-agnostic. Picked so
/// it can't collide with the `EntityId`-packed payload used by the
/// scene-tree reparent flow (entity generations are u32 and start at 1, so
/// any value with `0xKHPF` in the top half can only mean "asset tile").
pub(crate) const ASSET_DRAG_TAG: u64 = 0x4B48_5046_0000_0000; // "KHPF" in ASCII

/// Walks a `SceneNode` forest to find `entity`'s display name. Used by
/// the asset browser's drop receiver to compose a default `.kprefab`
/// filename without a round-trip through the live `World`.
fn entity_display_name(
    roots: &[khora_sdk::editor_ui::SceneNode],
    entity: khora_sdk::prelude::ecs::EntityId,
) -> Option<String> {
    fn walk(
        nodes: &[khora_sdk::editor_ui::SceneNode],
        target: khora_sdk::prelude::ecs::EntityId,
    ) -> Option<String> {
        for node in nodes {
            if node.entity == target {
                return Some(node.name.clone());
            }
            if let Some(found) = walk(&node.children, target) {
                return Some(found);
            }
        }
        None
    }
    walk(roots, entity)
}

/// Strips characters that aren't safe in cross-platform file names
/// (path separators, shell-meta, control chars). Falls back to an
/// underscore for runs of stripped chars so consecutive replacements
/// don't collapse into nothing.
fn sanitize_for_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_replacement = false;
    for ch in name.chars() {
        let safe = ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ' ');
        if safe {
            out.push(ch);
            last_was_replacement = false;
        } else if !last_was_replacement {
            out.push('_');
            last_was_replacement = true;
        }
    }
    let trimmed = out.trim_matches(&[' ', '.', '_'][..]).to_string();
    if trimmed.is_empty() {
        "prefab".to_string()
    } else {
        trimmed
    }
}

/// Number of low bits of `EditorState::asset_epoch` stamped into a drag
/// payload. Eight is plenty: the stamp only has to survive one drag, and the
/// epoch would have to advance 256 times mid-gesture to alias.
const DRAG_EPOCH_BITS: u32 = 8;
const DRAG_INDEX_MASK: u64 = (1 << (32 - DRAG_EPOCH_BITS)) - 1;

/// Packs an asset-list index plus a stamp of the epoch it was read at.
///
/// The index alone is not enough: it points into `EditorState::asset_entries`,
/// which `hot_reload::pump` rebuilds whenever a file appears, disappears or is
/// renamed on disk. Without the stamp, a rescan mid-drag silently retargets the
/// drop at whatever now occupies that slot.
pub(crate) fn pack_asset_drag(index: u32, epoch: u64) -> u64 {
    let stamp = (epoch & ((1 << DRAG_EPOCH_BITS) - 1)) << (32 - DRAG_EPOCH_BITS);
    ASSET_DRAG_TAG | stamp | (index as u64 & DRAG_INDEX_MASK)
}

/// Whether `payload` carries the asset tag, regardless of how stale it is.
///
/// Kept separate from [`unpack_asset_drag`] because the two answer different
/// questions: this one classifies the payload, the other resolves it. Routing
/// classification through the epoch check would make a stale asset drag look
/// like a packed `EntityId` and get handled as a reparent.
pub(crate) fn is_asset_drag(payload: u64) -> bool {
    payload & 0xFFFF_FFFF_0000_0000 == ASSET_DRAG_TAG
}

/// Unpacks an asset drag payload, rejecting it when the asset list changed
/// since the drag started.
pub(crate) fn unpack_asset_drag(payload: u64, current_epoch: u64) -> Option<u32> {
    if !is_asset_drag(payload) {
        return None;
    }
    let stamp = (payload >> (32 - DRAG_EPOCH_BITS)) & ((1 << DRAG_EPOCH_BITS) - 1);
    if stamp != current_epoch & ((1 << DRAG_EPOCH_BITS) - 1) {
        log::debug!("Asset drop ignored: the asset list changed during the drag");
        return None;
    }
    Some((payload & DRAG_INDEX_MASK) as u32)
}

/// Re-applies the original file's extension to a user-edited name when the
/// user omitted one (so renaming `crate.png` to `box` yields `box.png`, but
/// `box.jpg` is honoured verbatim).
fn ensure_extension(new_name: &str, old_name: &str) -> String {
    if new_name.contains('.') {
        return new_name.to_string();
    }
    match old_name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => format!("{new_name}.{ext}"),
        _ => new_name.to_string(),
    }
}

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
        match self.state.lock().ok().and_then(|s| s.project_folder.clone()) {
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
        ui.overlay_rect_filled([x, y], [w, h], with_alpha(theme.surface, 0.98), theme.radius_md);
        ui.overlay_rect_stroke([x, y], [w, h], with_alpha(accent, 0.9), theme.radius_md, 1.0);
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
            *rename_rect = Some([icon_x + 12.0, *y + 2.0, row_w - (icon_x - row_x) - 24.0, 18.0]);
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

const SIDEBAR_CATEGORIES: &[(AssetTileKind, &str, Icon)] = &[
    (AssetTileKind::Unknown, "All Assets", Icon::Database),
    (AssetTileKind::Scene, "Scenes", Icon::Film),
    (AssetTileKind::Mesh, "Meshes", Icon::Cube),
    (AssetTileKind::Texture, "Textures", Icon::Image),
    (AssetTileKind::Material, "Materials", Icon::Circle),
    (AssetTileKind::Audio, "Audio", Icon::Music),
    (AssetTileKind::Shader, "Shaders", Icon::Zap),
    (AssetTileKind::Script, "Scripts", Icon::Code),
];

impl EditorPanel for AssetBrowserPanel {
    fn id(&self) -> &str {
        "khora.editor.asset_browser"
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

        // Header action buttons. Trash deletes the current selection; Filter
        // and More open small context menus. Effects are collected here and
        // applied after the loop so the closures don't need `&mut self`.
        let mut header_delete_selected = false;
        let mut header_filter_pick: Option<Option<AssetTileKind>> = None;
        let mut header_new_folder = false;
        let mut header_refresh = false;
        let mut header_reveal_project = false;
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
            match salt {
                "ab-trash" if int.clicked => {
                    header_delete_selected = true;
                }
                "ab-filter" => {
                    ui.context_menu_last(&mut |menu| {
                        for (kind, label, _icon) in SIDEBAR_CATEGORIES {
                            if menu.button(label) {
                                header_filter_pick = Some(if *kind == AssetTileKind::Unknown {
                                    None
                                } else {
                                    Some(*kind)
                                });
                                menu.close_menu();
                            }
                        }
                    });
                }
                "ab-more" => {
                    ui.context_menu_last(&mut |menu| {
                        if menu.button("New Folder") {
                            header_new_folder = true;
                            menu.close_menu();
                        }
                        if menu.button("Refresh") {
                            header_refresh = true;
                            menu.close_menu();
                        }
                        if menu.button("Reveal Project in Explorer") {
                            header_reveal_project = true;
                            menu.close_menu();
                        }
                    });
                }
                _ => {}
            }
        }
        if let Some(pick) = header_filter_pick {
            self.selected_filter = pick;
        }
        if header_delete_selected {
            match self.selected_index.and_then(|i| self.flat.get(i)) {
                Some(asset) => {
                    self.confirm_delete = Some((asset.rel_path.clone(), false));
                }
                None => log::info!("Asset browser: nothing selected to delete"),
            }
        }
        if header_new_folder {
            let parent = self.current_folder.clone().unwrap_or_default();
            self.queue_new_folder(&parent);
        }
        if header_refresh {
            log::info!("Asset browser: refresh requested (hot-reload pump drives the rescan)");
        }
        if header_reveal_project {
            self.reveal_project();
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
        paint_text_size(ui, [px + 8.0, row_y], "FOLDERS", 10.0, theme.text_muted);
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
        self.render_breadcrumb(
            ui,
            grid_x + 12.0,
            crumb_y + 7.0,
            crumb_max_w,
            &theme,
            &project_folder,
        );

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
            .filter(|(_, a)| filter_text.is_empty() || a.name.to_lowercase().contains(&filter_text))
            .collect();

        // Drop target for entity drags from the scene tree. Registered
        // *before* the tile loop so the per-tile `interact_rect` calls
        // come later and take click priority — without this ordering
        // the grid-wide rect overlays the tiles and absorbs single
        // clicks (regression: tiles unselectable). The drop check
        // happens immediately, while `last_response` still points at
        // the grid rect; tiles registering after won't change the
        // already-read drop status.
        let drop_y = grid_inner_y;
        let drop_h = (body_y + body_h - grid_inner_y).max(0.0);
        let _drop_int =
            ui.interact_rect("ab-grid-drop", [grid_inner_x, drop_y, grid_inner_w, drop_h]);
        if let Some(payload) = ui.dnd_take_drop_payload() {
            if crate::panels::scene_tree::payload_is_entity(payload) {
                let entity = crate::panels::scene_tree::unpack_entity(payload);
                self.queue_save_entity_as_prefab(entity);
            }
        }

        // Right-click on the empty grid background. Bound to the same grid-wide
        // response as the drop target above; per-tile menus (registered later,
        // on top) take precedence when the cursor is over a tile.
        let mut bg_new_folder = false;
        let mut bg_reveal_current = false;
        let mut bg_refresh = false;
        ui.context_menu_last(&mut |menu| {
            if menu.button("New Folder") {
                bg_new_folder = true;
                menu.close_menu();
            }
            if menu.button("Reveal Current Folder") {
                bg_reveal_current = true;
                menu.close_menu();
            }
            if menu.button("Refresh") {
                bg_refresh = true;
                menu.close_menu();
            }
        });
        if bg_new_folder || bg_reveal_current {
            let cur = self.current_folder.clone().unwrap_or_default();
            if bg_new_folder {
                self.queue_new_folder(&cur);
            }
            if bg_reveal_current {
                self.reveal_in_explorer(&cur, true);
            }
        }
        if bg_refresh {
            log::info!("Asset browser: refresh requested (hot-reload pump drives the rescan)");
        }

        let tile_h = TILE_SIZE + 22.0;
        let mut to_select: Option<usize> = None;
        let mut to_activate: Option<usize> = None;
        let mut to_assign_material: Option<String> = None;
        let mut to_reveal: Option<usize> = None;
        let mut to_rename: Option<usize> = None;
        let mut to_duplicate: Option<String> = None;
        let mut to_delete: Option<String> = None;
        // Name + kind of the tile currently being dragged, so we can paint a
        // cursor-following ghost after the grid (a visible "I'm carrying this").
        let mut dragging_ghost: Option<(String, AssetTileKind)> = None;
        // A scene-tree entity dropped onto a tile → save it as a prefab here.
        let mut entity_drop: Option<khora_sdk::prelude::ecs::EntityId> = None;
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
            // Every tile is a drag source — the low 32 bits carry the index
            // into `EditorState::asset_entries`. The drop sink (viewport /
            // folder tree) dispatches by the asset's type.
            // Stamped with the epoch `flat` was built for — that is the epoch
            // `orig_idx` is an index into.
            ui.dnd_attach_drag_payload(pack_asset_drag(
                *orig_idx as u32,
                self.last_epoch.unwrap_or(0),
            ));
            if ui.is_last_item_dragged() {
                dragging_ghost = Some((asset.name.clone(), asset.asset_type));
            }
            // A scene-tree entity released ONTO this tile saves it as a prefab.
            // Tiles blanket the populated grid, so without this the entity would
            // land on a tile (top hovered) and the grid-wide drop rect below
            // would never see it.
            if let Some(payload) = ui.dnd_take_drop_payload() {
                if crate::panels::scene_tree::payload_is_entity(payload) {
                    entity_drop = Some(crate::panels::scene_tree::unpack_entity(payload));
                }
            }
            // Generic per-tile context menu (same idiom the scene tree uses
            // for entity actions). Material tiles keep their "Assign to
            // selected" entry on top of the shared actions.
            let idx = *orig_idx;
            let rel = asset.rel_path.clone();
            let is_material = asset.type_name == "material";
            ui.context_menu_last(&mut |menu| {
                if menu.button("Open") {
                    to_activate = Some(idx);
                    menu.close_menu();
                }
                if menu.button("Reveal in Explorer") {
                    to_reveal = Some(idx);
                    menu.close_menu();
                }
                if is_material && menu.button("Assign to selected") {
                    to_assign_material = Some(rel.clone());
                    menu.close_menu();
                }
                menu.separator();
                if menu.button("Rename") {
                    to_rename = Some(idx);
                    menu.close_menu();
                }
                if menu.button("Duplicate") {
                    to_duplicate = Some(rel.clone());
                    menu.close_menu();
                }
                if menu.button("Delete") {
                    to_delete = Some(rel.clone());
                    menu.close_menu();
                }
            });
            if interaction.double_clicked {
                to_activate = Some(*orig_idx);
            } else if interaction.clicked {
                to_select = Some(*orig_idx);
            }
        }
        // Cursor-following drag ghost — painted after the grid so it sits on top.
        if let Some((name, kind)) = dragging_ghost {
            if let Some(pos) = ui.pointer_position() {
                self.paint_drag_ghost(ui, pos, &name, kind, &theme);
            }
        }
        if let Some(entity) = entity_drop {
            self.queue_save_entity_as_prefab(entity);
        }
        if let Some(rel) = to_assign_material {
            if let Ok(mut state) = self.state.lock() {
                if state.selection.is_empty() {
                    log::warn!(
                        "Asset browser: assign material '{}' ignored — no entity selected",
                        rel
                    );
                } else {
                    state.pending_assign_material = Some(rel.clone());
                    log::info!("Asset browser: assigning material '{}' to selection", rel);
                }
            }
        }
        if let Some(idx) = to_reveal {
            if let Some(rel) = self.flat.get(idx).map(|a| a.rel_path.clone()) {
                self.reveal_in_explorer(&rel, false);
            }
        }
        if let Some(idx) = to_rename {
            // Seed the shared buffer with the current file name and switch this
            // tile into inline-edit mode; the overlay is drawn near the search
            // box handling at the end of `ui()`.
            if let Some(asset) = self.flat.get(idx) {
                self.rename_buffer = asset.name.clone();
                self.renaming_index = Some(idx);
                self.rename_focus_pending = true;
            }
        }
        if let Some(rel) = to_duplicate {
            if let Ok(mut state) = self.state.lock() {
                state.pending_duplicate_asset = Some(rel.clone());
            }
            log::info!("Asset browser: duplicating '{rel}'");
        }
        if let Some(rel) = to_delete {
            self.confirm_delete = Some((rel, false));
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

        // Inline tile-rename overlay: a text field painted over the renaming
        // tile's label. Committed on Enter, cancelled on Escape (the builder
        // doesn't surface focus-loss, so those two keys are the commit path).
        if let Some(rename_idx) = self.renaming_index {
            let slot = visible.iter().position(|(oi, _)| *oi == rename_idx).map(|pos| {
                let col = pos % cols;
                let row = pos / cols;
                let tx = grid_inner_x + col as f32 * (TILE_SIZE + TILE_GAP);
                let ty = grid_inner_y + row as f32 * (tile_h + TILE_GAP);
                [tx + 3.0, ty + TILE_SIZE - 2.0, TILE_SIZE - 6.0, 20.0]
            });
            // Snapshot the asset before taking the `&mut` buffer borrow.
            let asset = self.flat.get(rename_idx).cloned();
            match (slot, asset) {
                (Some(rect), Some(asset)) => {
                    let focus = self.rename_focus_pending;
                    let salt = format!("tile-rename-{rename_idx}");
                    let buf = &mut self.rename_buffer;
                    let event = ui.inline_text_field(rect, &salt, buf, focus);
                    self.rename_focus_pending = false;
                    match event {
                        InlineEditEvent::Committed => {
                            let edited = self.rename_buffer.trim().to_string();
                            if !edited.is_empty() {
                                let final_name = ensure_extension(&edited, &asset.name);
                                let new_rel = if asset.folder.is_empty() {
                                    final_name
                                } else {
                                    format!("{}/{}", asset.folder, final_name)
                                };
                                if new_rel != asset.rel_path {
                                    if let Ok(mut state) = self.state.lock() {
                                        state.pending_rename_asset =
                                            Some((asset.rel_path.clone(), new_rel));
                                    }
                                    log::info!("Asset browser: renaming '{}'", asset.rel_path);
                                }
                            }
                            self.renaming_index = None;
                        }
                        InlineEditEvent::Cancelled => self.renaming_index = None,
                        _ => {}
                    }
                }
                // The tile was filtered out or removed — abandon the rename.
                _ => self.renaming_index = None,
            }
        }

        let search_filter_ref = &mut self.search_filter;
        ui.region_at(
            "asset-browser-search",
            [search_x + 20.0, search_y, search_w - 22.0, 22.0],
            &mut |ui_inner| {
                ui_inner.text_edit_singleline(search_filter_ref);
            },
        );

        // Painted last so it sits above the grid it is asking about.
        self.render_delete_confirmation(ui, panel_rect, &theme);
    }
}
