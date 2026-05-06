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

//! Project-scoped Virtual File System for the editor.
//!
//! Wraps the `khora-io` `AssetService` + `AssetWatcher` together with a
//! `FileLoader` rooted at `<project>/assets/`. This is the single I/O entry
//! point used by `scene_io`, the asset browser, and any other editor code
//! that needs to read or write project content.

use anyhow::{Context, Result};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::renderer::api::scene::Mesh;
use khora_sdk::{
    AssetChangeEvent, AssetService, AssetWatcher, AssetWriter, FileLoader, FileSystemResolver,
    IndexBuilder, MeshDispatcher, MetricsRegistry, SoundData, SymphoniaDecoder,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// All project I/O, in one place.
///
/// `open` performs a recursive scan of `<root>/assets/`, builds an in-memory
/// VFS index (UUIDs derived from forward-slash relative paths via
/// `AssetUUID::new_v5`), constructs the `AssetService` with every default
/// decoder registered, and arms a recursive filesystem watcher for hot
/// reload. UUIDs match what a future pack-builder would produce, so the
/// dev/release transparency promise of the VFS is preserved.
pub struct ProjectVfs {
    pub root: PathBuf,
    pub assets_root: PathBuf,
    pub asset_service: AssetService,
    pub watcher: AssetWatcher,
    file_loader: FileLoader,
}

impl ProjectVfs {
    /// Opens (or creates) `<root>/assets/`, builds the VFS, and arms the
    /// watcher. Tolerates fresh projects whose `assets/` directory hasn't
    /// been populated yet — the resulting service has zero indexed assets,
    /// which is exactly what the asset browser will show.
    pub fn open(root: PathBuf, metrics: Arc<MetricsRegistry>) -> Result<Self> {
        let assets_root = root.join("assets");
        std::fs::create_dir_all(&assets_root).with_context(|| {
            format!(
                "Failed to ensure project assets directory exists: {}",
                assets_root.display()
            )
        })?;

        let index_bytes = IndexBuilder::new(&assets_root)
            .build_index_bytes()
            .context("Failed to build initial project asset index")?;

        let file_loader = FileLoader::new(&assets_root);
        // Note: we hand a *clone* (a fresh FileLoader) to the AssetService so
        // that we keep our own `file_loader` available to implement
        // `AssetWriter` for scene saves. They both read/write the same root,
        // so this is consistent.
        let io = Box::new(FileLoader::new(&assets_root));
        let mut asset_service = AssetService::new(&index_bytes, io, metrics)
            .context("Failed to construct AssetService")?;

        // texture + font auto-register via inventory.
        asset_service.register_inventory_decoders();
        // audio + mesh are explicitly chosen by the consumer (see
        // doctrine in decoders/{audio,mesh}/mod.rs).
        asset_service.register_decoder::<SoundData>("audio", SymphoniaDecoder);
        // Mesh dispatch: gltf URIs (external `.bin` / texture buffers) are
        // resolved relative to the project's `assets/` root. Authors place
        // referenced resources at project-relative paths (e.g.
        // `meshes/character/diffuse.png`) and reference them with that same
        // path inside the gltf — this diverges from the strict gltf-spec
        // "URIs are relative to the gltf file" but matches the rest of the
        // VFS's path convention. `.glb` and `.obj` files are self-contained
        // and don't go through the resolver at all.
        let gltf_resolver = Arc::new(FileSystemResolver::new(&assets_root));
        asset_service.register_decoder::<Mesh>("mesh", MeshDispatcher::new(gltf_resolver));

        let watcher = AssetWatcher::new(&assets_root)
            .context("Failed to start asset watcher (filesystem hot reload)")?;

        log::info!(
            "ProjectVfs opened: {} assets indexed under {}, watcher armed.",
            asset_service.vfs().asset_count(),
            assets_root.display()
        );

        Ok(Self {
            root,
            assets_root,
            asset_service,
            watcher,
            file_loader,
        })
    }

    /// Re-walks `<root>/assets/` and atomically swaps the VFS index. Called
    /// after a scene save (so the new file shows up) and from the
    /// hot-reload pump on Created/Removed events.
    pub fn rebuild_index(&mut self) -> Result<()> {
        let bytes = IndexBuilder::new(&self.assets_root)
            .build_index_bytes()
            .context("Failed to rebuild project asset index")?;
        self.asset_service.reindex(&bytes)?;
        Ok(())
    }

    /// Reads `<root>/project.json` as untyped JSON. Project metadata isn't an
    /// asset (lives outside `assets/`) so it deliberately bypasses the VFS.
    pub fn read_project_json(&self) -> Result<serde_json::Value> {
        let path = self.root.join("project.json");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Writes `bytes` to `<root>/assets/<rel_path>`. Creates intermediate
    /// directories as needed. Caller is responsible for calling
    /// [`Self::rebuild_index`] afterwards if the new path needs to be
    /// resolvable through the VFS in the same frame.
    pub fn write_asset(&self, rel_path: &Path, bytes: &[u8]) -> Result<()> {
        self.file_loader.write_bytes(rel_path, bytes)
    }

    /// Returns the UUID for a relative-path-with-forward-slashes string,
    /// regardless of whether the path currently exists on disk. Used by
    /// `scene_io` to look up scenes by canonical path even before a save
    /// has triggered a reindex.
    pub fn uuid_for_rel_path(rel_path_fwd_slash: &str) -> AssetUUID {
        AssetUUID::new_v5(rel_path_fwd_slash)
    }

    /// Drains pending hot-reload events. Convenience wrapper so callers
    /// don't need to reach through `pvfs.watcher`.
    pub fn poll_changes(&self) -> Vec<AssetChangeEvent> {
        self.watcher.poll()
    }
}
