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

//! A file's parts in the browser: a glTF's scenes, meshes, materials and
//! textures are listed behind their file's tile, on a band of their own, and
//! follow their file — they are never renamed, moved, deleted or duplicated
//! on their own.

use khora_sdk::editor_ui::{UiBuilder, UiTheme};
use khora_sdk::khora_core::asset::AssetUUID;

use super::{AssetBrowserPanel, FlatAsset, TILE_SIZE};

impl AssetBrowserPanel {
    /// The tiles of the grid, in order, as indices into `flat` (and so into
    /// `EditorState::asset_entries`): the assets of the current folder that
    /// the kind filter and the search keep. A sub-asset is no tile of its
    /// folder: it follows its file's tile, and only while that file is
    /// expanded.
    ///
    /// The kind filter and the search see a file's parts too: a file shows
    /// when it or one of its parts is kept, and opened it lists only the
    /// parts kept (all of them when the search names the file itself).
    pub(super) fn grid_tiles(&self) -> Vec<usize> {
        let filter_text = self.search_filter.to_lowercase();
        let current_folder = self.current_folder.clone().unwrap_or_default();
        let kind_kept = |asset: &FlatAsset| {
            self.selected_filter
                .is_none_or(|kind| asset.asset_type == kind)
        };
        let named = |asset: &FlatAsset| {
            filter_text.is_empty() || asset.name.to_lowercase().contains(&filter_text)
        };
        let mut tiles = Vec::new();
        for (idx, asset) in self.flat.iter().enumerate() {
            if asset.container.is_some() {
                continue;
            }
            // When a folder is selected, only show assets that live *within*
            // it (recursive — `assets/props` matches `assets/props/crates/a.png`).
            let folder_kept = current_folder.is_empty()
                || asset.folder == current_folder
                || asset.folder.starts_with(&format!("{current_folder}/"));
            if !folder_kept {
                continue;
            }
            let file_named = named(asset);
            let parts: Vec<usize> = self
                .parts_of(idx)
                .iter()
                .copied()
                .filter(|&part| kind_kept(&self.flat[part]))
                .filter(|&part| file_named || named(&self.flat[part]))
                .collect();
            let file_kept = kind_kept(asset) && file_named;
            if !file_kept && parts.is_empty() {
                continue;
            }
            tiles.push(idx);
            if self.open_containers.contains(&asset.uuid) {
                tiles.extend(parts);
            }
        }
        tiles
    }

    /// The parts of the file at `flat[idx]`, in index order.
    pub(super) fn parts_of(&self, idx: usize) -> &[usize] {
        self.flat
            .get(idx)
            .and_then(|asset| self.parts_by_file.get(&asset.uuid))
            .map_or(&[], Vec::as_slice)
    }

    /// How many sub-assets the file at `flat[idx]` holds — the count its
    /// disclosure toggle shows. `0` for an asset holding none.
    pub(super) fn sub_count(&self, idx: usize) -> usize {
        self.parts_of(idx).len()
    }

    /// Opens or closes the file at `flat[idx]`: its sub-assets follow its
    /// tile in the grid while it is open. Every file starts closed.
    pub(super) fn toggle_container(&mut self, idx: usize) {
        if self.sub_count(idx) == 0 {
            return;
        }
        let Some(file) = self.flat.get(idx).map(|asset| asset.uuid) else {
            return;
        };
        if !self.open_containers.remove(&file) {
            self.open_containers.insert(file);
        }
    }

    /// Whether the tile at `flat[idx]` offers the file operations — rename,
    /// move, delete, duplicate. A sub-asset does not: it follows its file.
    pub(super) fn file_ops_offered(&self, idx: usize) -> bool {
        self.flat
            .get(idx)
            .is_some_and(|asset| asset.container.is_none())
    }

    /// Paints the band an open file and its parts sit on, behind their
    /// tiles: `visible` are the grid's tiles, `rect` gives the slot of the
    /// `i`-th one.
    pub(super) fn paint_part_bands(
        &self,
        ui: &mut dyn UiBuilder,
        visible: &[(usize, FlatAsset)],
        rect: impl Fn(usize) -> [f32; 4],
        theme: &UiTheme,
    ) {
        let mut run: Vec<[f32; 4]> = Vec::new();
        let mut run_file: Option<AssetUUID> = None;
        for (i, (_, asset)) in visible.iter().enumerate() {
            let file = match asset.container {
                Some(file) => Some(file),
                None if self.open_containers.contains(&asset.uuid) => Some(asset.uuid),
                None => None,
            };
            if file != run_file || asset.container.is_none() {
                crate::widgets::tile::paint_parts_band(ui, &run, theme);
                run.clear();
            }
            run_file = file;
            if file.is_some() {
                run.push(rect(i));
            }
        }
        crate::widgets::tile::paint_parts_band(ui, &run, theme);
    }

    /// Paints the disclosure chip of the tile of `flat[idx]` at `origin`,
    /// when the file holds parts, and reports whether it was clicked.
    pub(super) fn paint_part_chip(
        &self,
        ui: &mut dyn UiBuilder,
        idx: usize,
        origin: [f32; 2],
        theme: &UiTheme,
    ) -> bool {
        let count = self.sub_count(idx);
        let open = self
            .flat
            .get(idx)
            .is_some_and(|asset| self.open_containers.contains(&asset.uuid));
        count > 0
            && crate::widgets::tile::paint_parts_chip(
                ui,
                &format!("tile-{idx}-parts"),
                origin,
                TILE_SIZE,
                count,
                open,
                theme,
            )
    }

    /// Asks to delete the selected tile's file — never a part on its own,
    /// which goes with its file.
    pub(super) fn ask_to_delete_selected(&mut self) {
        match self
            .selected_index
            .and_then(|i| self.flat.get(i).map(|a| (i, a)))
        {
            // A part goes with its file, never on its own.
            Some((i, asset)) if !self.file_ops_offered(i) => log::info!(
                "Asset browser: '{}' is a part of its file; delete the file instead",
                asset.name
            ),
            Some((_, asset)) => {
                self.confirm_delete = Some((asset.rel_path.clone(), false));
            }
            None => log::info!("Asset browser: nothing selected to delete"),
        }
    }
}
