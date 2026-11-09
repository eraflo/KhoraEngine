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
//! Wraps the `khora-io` `AssetService` and a subscription to an `AssetWatch` together with a
//! `FileLoader` rooted at `<project>/assets/`. This is the single I/O entry
//! point used by `scene_io`, the asset browser, and any other editor code
//! that needs to read or write project content.

pub use crate::after_index::{AfterIndex, Landed};
use anyhow::{bail, Context, Result};
use khora_sdk::khora_core::asset::{asset_key, AssetUUID};
use khora_sdk::khora_core::physics::PhysicsSettings;
use khora_sdk::{
    AssetChangeEvent, AssetIdRegistry, AssetService, AssetWatch, AssetWriter, FileLoader,
    IndexBuilder, MetricsRegistry,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
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
    /// The project's assets: shared with the engine (see
    /// [`Self::share_assets`]), so the editor and the frame read one index
    /// and one cache.
    asset_service: Arc<Mutex<AssetService>>,
    /// Where its changes are read from: the engine's shared watch — one
    /// subscriber among the script and shader pumps — or, opened on its own,
    /// a watch of its own.
    watch: Arc<AssetWatch>,
    file_loader: FileLoader,
    /// Stable-identity registry (`<root>/.khora/asset-registry.ron`). The editor
    /// is the sole writer: file operations freeze/rename/remove entries so an
    /// asset keeps its UUID across renames and every reference keeps resolving.
    registry: AssetIdRegistry,
    /// Moves on every write and every reindex: what was read from the
    /// project's files before is stale once it moves.
    generation: std::sync::atomic::AtomicU64,
    /// The catalog a script's prefab `Spawn` is checked against, kept to the
    /// index: refilled at every reindex.
    prefab_catalog: Option<khora_sdk::khora_data::scene::PrefabCatalog>,
    /// The engine's index hub, which this project's reindexes are filed
    /// with; `None` reindexes on the frame.
    indexes: Option<Arc<khora_sdk::IndexHub>>,
    /// The reindex in flight, and what waits for it to land.
    awaiting: Option<(khora_sdk::IndexTicket, Vec<AfterIndex>)>,
    /// What waits for the reindex filed once the one in flight lands — a
    /// write made while it was built — when one was asked for.
    next: Option<Vec<AfterIndex>>,
    /// The materials to assign that landed while the scene was played or
    /// restored: assigned to the edited scene once it is back.
    held: Vec<AfterIndex>,
}

impl ProjectVfs {
    /// Opens (or creates) `<root>/assets/`, builds the VFS, and arms the
    /// watcher. Tolerates fresh projects whose `assets/` directory hasn't
    /// been populated yet — the resulting service has zero indexed assets,
    /// which is exactly what the asset browser will show.
    #[cfg(test)]
    pub fn open(root: PathBuf, metrics: Arc<MetricsRegistry>) -> Result<Self> {
        Self::open_watched(root, metrics, Arc::new(AssetWatch::default()))
    }

    /// Opens `<root>/assets/` as [`Self::open`] does, reading its changes
    /// from the engine's shared `watch` — one subscriber among the script and
    /// shader pumps — rather than from a watcher of its own.
    /// [`Self::poll_changes`] then reports the changes under this project's
    /// assets root only.
    pub fn open_watched(
        root: PathBuf,
        metrics: Arc<MetricsRegistry>,
        watch: Arc<AssetWatch>,
    ) -> Result<Self> {
        let assets_root = root.join("assets");
        std::fs::create_dir_all(&assets_root).with_context(|| {
            format!(
                "Failed to ensure project assets directory exists: {}",
                assets_root.display()
            )
        })?;

        // The identity registry lives at the project root (sibling of
        // `assets/`), so it is never scanned, watched, or packed. Missing file →
        // empty registry → every path keeps its `new_v5` default.
        let registry = AssetIdRegistry::load(&root);
        // Materialize the file on first open so it's visible in the project and
        // confirms persistence is wired, even before the first rename.
        if let Err(e) = registry.ensure_file() {
            log::warn!("Could not create asset identity registry file: {e}");
        }

        let asset_service = AssetService::open_loose_files(&root, &registry, metrics)
            .context("Failed to construct AssetService")?;
        // Our own loader over the same root, kept to implement `AssetWriter`
        // for scene saves.
        let file_loader = FileLoader::new(&assets_root);

        // Armed here too — watching is idempotent — so a project opened on
        // its own still hears its files.
        watch
            .watch(&assets_root)
            .context("Failed to watch the project's assets (filesystem hot reload)")?;

        log::info!(
            "ProjectVfs opened: {} assets indexed under {}, watcher armed.",
            asset_service.index().asset_count(),
            assets_root.display()
        );

        Ok(Self {
            root,
            assets_root,
            asset_service: Arc::new(Mutex::new(asset_service)),
            watch,
            file_loader,
            registry,
            generation: std::sync::atomic::AtomicU64::new(0),
            prefab_catalog: None,
            indexes: None,
            awaiting: None,
            next: None,
            held: Vec::new(),
        })
    }

    /// Opens the project at `root` over `service`, the engine's shared asset
    /// service, which an `Open` filed with the engine's index hub filled off
    /// the frame. As [`Self::open_watched`] does, it ensures `<root>/assets/`
    /// exists, loads the identity registry (its file made), builds the
    /// writer and arms `watch`; it walks nothing: the index is the service's.
    pub fn over_service(
        root: PathBuf,
        service: Arc<Mutex<AssetService>>,
        watch: Arc<AssetWatch>,
    ) -> Result<Self> {
        let assets_root = root.join("assets");
        std::fs::create_dir_all(&assets_root).with_context(|| {
            format!(
                "Failed to ensure project assets directory exists: {}",
                assets_root.display()
            )
        })?;
        // The identity registry, made on first open, as `open_watched` does.
        let registry = AssetIdRegistry::load(&root);
        if let Err(e) = registry.ensure_file() {
            log::warn!("Could not create asset identity registry file: {e}");
        }
        let file_loader = FileLoader::new(&assets_root);
        watch
            .watch(&assets_root)
            .context("Failed to watch the project's assets (filesystem hot reload)")?;
        Ok(Self {
            root,
            assets_root,
            asset_service: service,
            watch,
            file_loader,
            registry,
            generation: std::sync::atomic::AtomicU64::new(0),
            prefab_catalog: None,
            indexes: None,
            awaiting: None,
            next: None,
            held: Vec::new(),
        })
    }

    /// The project's asset service, locked for the length of a statement.
    pub fn assets(&self) -> MutexGuard<'_, AssetService> {
        self.asset_service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Serves this project's assets through `shared` from now on: its
    /// service moves into `shared`, which the engine holds — the asset
    /// resolver then loads the meshes, materials and textures the editor's
    /// entities reference, from the same index and cache the editor reads.
    pub fn share_assets(&mut self, shared: &Arc<Mutex<AssetService>>) {
        if Arc::ptr_eq(&self.asset_service, shared) {
            return;
        }
        {
            let mut own = self.assets();
            let mut target = shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::swap(&mut *own, &mut *target);
            // A uuid is its path unless the registry froze it, so another
            // project's `textures/a.png` is the same uuid: what the engine
            // took from the service moving out may be stale now.
            target.announce_replacing(&own);
            // The engine's `loading` family takes from the channel the service
            // moving out filed on: the one moving in files there now.
            target.adopt_decode_requests(&mut own);
        }
        self.asset_service = Arc::clone(shared);
    }

    /// Keeps `catalog` to this project's prefabs from now on.
    pub fn follow_prefab_catalog(&mut self, catalog: khora_sdk::khora_data::scene::PrefabCatalog) {
        catalog.replace(khora_sdk::prefab_catalog(&self.assets()));
        self.prefab_catalog = Some(catalog);
    }

    /// Moves on every write and every reindex: what was read from the
    /// project's files under another generation may be stale.
    pub fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn moved_on(&self) {
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Re-walks `<root>/assets/` and swaps the VFS index, on the frame: what
    /// [`Self::reindex`] does without an engine, and what the setup-time
    /// scene load does.
    pub fn rebuild_index(&mut self) -> Result<()> {
        let bytes = IndexBuilder::new(&self.assets_root)
            .with_registry(&self.registry)
            .build_index_bytes()
            .context("Failed to rebuild project asset index")?;
        self.assets().reindex(&bytes)?;
        if let Some(catalog) = &self.prefab_catalog {
            catalog.replace(khora_sdk::prefab_catalog(&self.assets()));
        }
        self.moved_on();
        Ok(())
    }

    /// Takes in a reindex done off the frame, once it landed in the engine's
    /// asset service: the prefab catalog is refilled from the new index, and
    /// the project's generation moves.
    pub fn index_landed(&mut self) {
        if let Some(catalog) = &self.prefab_catalog {
            catalog.replace(khora_sdk::prefab_catalog(&self.assets()));
        }
        self.moved_on();
    }

    /// Files this project's reindexes with `hub`, the engine's index hub,
    /// from now on: they are built off the frame and land at a frame's
    /// boundary.
    pub fn index_with(&mut self, hub: Arc<khora_sdk::IndexHub>) {
        if self
            .indexes
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, &hub))
        {
            return;
        }
        self.indexes = Some(hub);
    }

    /// Keeps `actions` until the edited scene is back.
    pub fn hold(&mut self, actions: Vec<AfterIndex>) {
        self.held.extend(actions);
    }

    /// The actions kept until the edited scene was back.
    pub fn release(&mut self) -> Vec<AfterIndex> {
        std::mem::take(&mut self.held)
    }

    /// The reindex in flight, if any.
    pub fn reindexing(&self) -> Option<khora_sdk::IndexTicket> {
        self.awaiting.as_ref().map(|(ticket, _)| *ticket)
    }

    /// Indexes the project again after a write: with a hub and nothing in
    /// flight, files a reindex and keeps its ticket; while one is in flight,
    /// the write is indexed by the reindex filed once it lands; without a
    /// hub, rebuilds on the frame.
    pub fn reindex(&mut self) -> Result<()> {
        let Some(indexes) = self.indexes.clone() else {
            return self.rebuild_index();
        };
        if self.awaiting.is_some() {
            // The walk in flight may have begun before this write.
            self.next.get_or_insert_with(Vec::new);
            return Ok(());
        }
        let ticket = indexes.file(self.root.clone(), khora_sdk::IndexKind::Reindex);
        self.awaiting = Some((ticket, Vec::new()));
        Ok(())
    }

    /// Makes `action` wait for the reindex that will see the last write.
    /// With nothing in flight, it is returned, to run now.
    pub fn after_index(&mut self, action: AfterIndex) -> Option<AfterIndex> {
        // The material saved last is the one its entity wears.
        if let Some((_, waiting)) = &mut self.awaiting {
            waiting.retain(|waiting| !action.assigns_as(waiting));
        }
        if let Some(next) = &mut self.next {
            next.retain(|waiting| !action.assigns_as(waiting));
        }
        self.held.retain(|waiting| !action.assigns_as(waiting));
        if let Some(next) = &mut self.next {
            next.push(action);
            return None;
        }
        if let Some((_, waiting)) = &mut self.awaiting {
            waiting.push(action);
            return None;
        }
        Some(action)
    }

    /// Takes the outcome of the reindex in flight, once it landed: on `Ok`
    /// the project takes it in; the next reindex is filed if one was asked
    /// for. `None` while it is in flight, or when none is.
    ///
    /// The actions that waited for a reindex that failed are dropped: what
    /// they needed is not in the index.
    pub fn land(&mut self) -> Option<Landed> {
        let (ticket, _) = self.awaiting.as_ref()?;
        let outcome = self.indexes.as_ref()?.take_outcome(*ticket)?;
        let (_, waiting) = self.awaiting.take()?;
        let actions = match &outcome {
            Ok(()) => {
                self.index_landed();
                waiting
            }
            Err(why) => {
                if !waiting.is_empty() {
                    log::warn!(
                        "{} editor action(s) dropped: the project's reindex failed: {why}",
                        waiting.len()
                    );
                }
                Vec::new()
            }
        };
        if let Some(next) = self.next.take() {
            if let Some(indexes) = self.indexes.clone() {
                let ticket = indexes.file(self.root.clone(), khora_sdk::IndexKind::Reindex);
                self.awaiting = Some((ticket, next));
            }
        }
        Some(Landed { outcome, actions })
    }

    /// The project's physics settings: the `physics` section of its
    /// `project.json`, or the defaults when it has none. A section that does
    /// not read (a zero or negative step) is reported and the defaults kept.
    pub fn physics_settings(&self) -> PhysicsSettings {
        crate::project_physics::project_physics(&self.root)
    }

    /// Writes `bytes` to `<root>/assets/<rel_path>`. Creates intermediate
    /// directories as needed. Caller is responsible for calling
    /// [`Self::rebuild_index`] afterwards if the new path needs to be
    /// resolvable through the VFS in the same frame.
    pub fn write_asset(&self, rel_path: &Path, bytes: &[u8]) -> Result<()> {
        let written = self.file_loader.write_bytes(rel_path, bytes);
        self.moved_on();
        written
    }

    /// Resolves a forward-slash relative path to its UUID through the identity
    /// registry: the frozen identity if the asset has been renamed, otherwise
    /// the `new_v5` default. It must be used whenever an asset the user could
    /// have renamed is looked up by path (scene / prefab / material load).
    pub fn resolve_uuid(&self, rel_path_fwd_slash: &str) -> AssetUUID {
        self.registry.resolve(rel_path_fwd_slash)
    }

    /// Absolute path of an asset given its forward-slash relative path.
    fn abs_of(&self, rel_fwd: &str) -> PathBuf {
        let mut p = self.assets_root.clone();
        for seg in rel_fwd.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }

    /// Creates an (empty) folder under `assets/` at `rel_dir` (forward-slash).
    /// Empty folders are surfaced by [`Self::list_dirs`] since the VFS itself
    /// only indexes files.
    pub fn create_folder(&mut self, rel_dir: &str) -> Result<()> {
        let abs = self.abs_of(rel_dir);
        std::fs::create_dir_all(&abs)
            .with_context(|| format!("Failed to create folder {}", abs.display()))?;
        Ok(())
    }

    /// Renames/moves an asset from `old_rel` to `new_rel` (both forward-slash,
    /// under `assets/`), **freezing its UUID** so every existing reference keeps
    /// resolving. Fails if the destination already exists. Rebuilds the index.
    pub fn rename_asset(&mut self, old_rel: &str, new_rel: &str) -> Result<()> {
        if old_rel == new_rel {
            return Ok(());
        }
        let old_abs = self.abs_of(old_rel);
        let new_abs = self.abs_of(new_rel);
        if !old_abs.exists() {
            bail!("Source asset does not exist: {}", old_abs.display());
        }
        if new_abs.exists() {
            bail!("Destination already exists: {}", new_abs.display());
        }
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        std::fs::rename(&old_abs, &new_abs).with_context(|| {
            format!(
                "Failed to move {} → {}",
                old_abs.display(),
                new_abs.display()
            )
        })?;

        // Freeze identity across the move, then persist. In-memory update must
        // precede `rebuild_index` so the new path resolves to the frozen UUID.
        self.registry.rename(old_rel, new_rel);
        if let Err(e) = self.registry.save() {
            log::error!("Failed to persist asset registry after rename: {e}");
        }
        self.reindex()
    }

    /// Moves an asset into `dest_dir` (forward-slash folder, `""` = assets root),
    /// keeping its file name. Thin wrapper over [`Self::rename_asset`].
    pub fn move_asset(&mut self, src_rel: &str, dest_dir: &str) -> Result<()> {
        let file_name = src_rel.rsplit('/').next().unwrap_or(src_rel);
        let new_rel = if dest_dir.is_empty() {
            file_name.to_string()
        } else {
            format!("{}/{}", dest_dir.trim_end_matches('/'), file_name)
        };
        self.rename_asset(src_rel, &new_rel)
    }

    /// Sends an asset to the OS recycle bin (reversible), drops its registry
    /// entry, and rebuilds the index.
    pub fn delete_to_trash(&mut self, rel: &str) -> Result<()> {
        let abs = self.abs_of(rel);
        if !abs.exists() {
            bail!("Asset does not exist: {}", abs.display());
        }
        trash::delete(&abs)
            .with_context(|| format!("Failed to move {} to the recycle bin", abs.display()))?;
        self.registry.remove(rel);
        if let Err(e) = self.registry.save() {
            log::error!("Failed to persist asset registry after delete: {e}");
        }
        self.reindex()
    }

    /// Duplicates an asset next to itself with a unique ` copy` suffix. The copy
    /// is a **new asset** (no registry entry → fresh `new_v5` identity). Returns
    /// the new forward-slash relative path.
    pub fn duplicate_asset(&mut self, rel: &str) -> Result<String> {
        let src_abs = self.abs_of(rel);
        if !src_abs.exists() {
            bail!("Asset does not exist: {}", src_abs.display());
        }
        let (dir, file) = match rel.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f.to_string()),
            None => (String::new(), rel.to_string()),
        };
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (file.clone(), String::new()),
        };
        // Find the first free `stem copy`, `stem copy 2`, … name.
        let mut new_rel = String::new();
        for n in 1..10_000 {
            let suffix = if n == 1 {
                " copy".to_string()
            } else {
                format!(" copy {n}")
            };
            let candidate_file = format!("{stem}{suffix}{ext}");
            let candidate = if dir.is_empty() {
                candidate_file
            } else {
                format!("{dir}/{candidate_file}")
            };
            if !self.abs_of(&candidate).exists() {
                new_rel = candidate;
                break;
            }
        }
        if new_rel.is_empty() {
            bail!("Could not find a free duplicate name for {rel}");
        }
        std::fs::copy(&src_abs, self.abs_of(&new_rel))
            .with_context(|| format!("Failed to duplicate {}", src_abs.display()))?;
        self.reindex()?;
        Ok(new_rel)
    }

    /// Creates a new file at `rel` (forward-slash under `assets/`) holding
    /// `bytes`, its folders too. Refuses an existing file, an empty path, and any
    /// `..` / `.` / empty segment or absolute path. Rebuilds the index.
    pub fn create_asset(&mut self, rel: &str, bytes: &[u8]) -> Result<()> {
        // `abs_of` drops empty segments and joins the rest as they are, so the
        // path is checked here: a `..`, a drive or a backslash would put the
        // file somewhere other than where the author was shown.
        let stays_under_assets = !rel.is_empty()
            && rel.split('/').all(|segment| {
                !matches!(segment, "" | "." | "..") && !segment.contains(['\\', ':'])
            });
        if !stays_under_assets {
            bail!("Not a path under assets/: '{rel}'");
        }
        let abs = self.abs_of(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        // `create_new` refuses an existing file in the same call that opens it,
        // so nothing an author wrote is ever replaced.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&abs)
            .with_context(|| format!("Failed to create {}", abs.display()))?;
        std::io::Write::write_all(&mut file, bytes)
            .with_context(|| format!("Failed to write {}", abs.display()))?;
        drop(file);
        self.reindex()
    }

    /// Lists every directory under `assets/` as forward-slash relative paths
    /// (sorted, excluding the root). The VFS only indexes files, so empty
    /// folders — including freshly created ones — are only visible through this.
    pub fn list_dirs(&self) -> Vec<String> {
        let mut dirs = Vec::new();
        for entry in walkdir::WalkDir::new(&self.assets_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_dir() {
                continue;
            }
            let rel = match entry.path().strip_prefix(&self.assets_root) {
                Ok(r) if !r.as_os_str().is_empty() => r,
                _ => continue,
            };
            // Skip the engine's own `.khora` project dir if it ever sits inside.
            let rel_fwd = asset_key(rel);
            if rel_fwd.starts_with('.') {
                continue;
            }
            dirs.push(rel_fwd);
        }
        dirs.sort();
        dirs
    }

    /// Drains pending hot-reload events. Convenience wrapper so callers
    /// don't need to reach through `pvfs.watcher`.
    pub fn poll_changes(&self) -> Vec<AssetChangeEvent> {
        let own = self.assets_root.canonicalize().ok();
        self.watch
            .poll_for("editor_vfs")
            .into_iter()
            .filter(|event| {
                *event.root == *self.assets_root
                    || own.is_some() && event.root.canonicalize().ok() == own
            })
            .collect()
    }
}

/// The project's prefabs, read through its VFS: a prefab is the scene file
/// its asset id names. Locks the VFS for each read, so it can be handed to a
/// load or a save that does not hold it.
pub struct ProjectPrefabs<'a>(pub &'a std::sync::Mutex<ProjectVfs>);

impl khora_sdk::khora_data::scene::PrefabSource for ProjectPrefabs<'_> {
    fn prefab(
        &self,
        id: AssetUUID,
    ) -> std::result::Result<khora_sdk::khora_data::scene::SceneRecord, String> {
        self.0
            .lock()
            .map_err(|_| "the project VFS is poisoned".to_owned())?
            .prefab_record(id)
    }
}

impl ProjectVfs {
    /// The asset id of the file a write to `rel_path` lands on.
    ///
    /// Not simply the id of the path as typed: on a file system that ignores
    /// case, `Turret.kprefab` overwrites `turret.kprefab`, whose id is another.
    /// The file already there, under the name it has, is the one that counts.
    pub fn written_asset_id(&self, rel_path_fwd_slash: &str) -> AssetUUID {
        let existing = std::fs::canonicalize(self.assets_root.join(rel_path_fwd_slash))
            .ok()
            .zip(std::fs::canonicalize(&self.assets_root).ok())
            .and_then(|(file, root)| {
                file.strip_prefix(&root)
                    .ok()
                    .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            });
        self.resolve_uuid(existing.as_deref().unwrap_or(rel_path_fwd_slash))
    }

    /// The file of the asset known as `id`, relative to `assets/`, as the
    /// index knows it.
    pub fn rel_path_of(&self, id: AssetUUID) -> Option<String> {
        let assets = self.assets();
        let source = &assets.index().get_metadata(&id)?.source_path;
        let rel = source.strip_prefix(&self.assets_root).unwrap_or(source);
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    /// Whether `id` names a part of a file (a glTF's scene, mesh,
    /// material…) rather than a file of its own: it shares its file's path
    /// and is never written.
    pub fn is_part_of_a_file(&self, id: AssetUUID) -> bool {
        self.assets()
            .index()
            .get_metadata(&id)
            .and_then(|metadata| metadata.variants.get("default"))
            .is_some_and(|source| matches!(source, khora_sdk::AssetSource::Sub { .. }))
    }

    /// The record of the prefab known as `id`.
    ///
    /// A `.kprefab` file is read afresh each time, as the editor rewrites it
    /// in place; a prefab inside another file (a glTF scene) is decoded
    /// through the asset service, whose cache the file's hot reload clears.
    pub fn prefab_record(
        &mut self,
        id: AssetUUID,
    ) -> std::result::Result<khora_sdk::khora_data::scene::SceneRecord, String> {
        if self.is_part_of_a_file(id) {
            return khora_sdk::read_prefab(&mut self.assets(), id);
        }
        let bytes = self
            .assets()
            .load_raw(&id)
            .map_err(|error| format!("{error:#}"))?;
        let file =
            khora_sdk::SceneFile::from_bytes(&bytes).map_err(|error| format!("{error:?}"))?;
        khora_sdk::khora_data::scene::read_scene_file(&file).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::khora_core::renderer::api::gpu_scene::Mesh;

    /// One file per asset type the index builder knows. The bytes are not a
    /// valid asset of any kind: the probe only asks whether a decoder is
    /// registered for the type, not whether it accepts the file.
    const PROBES: &[&str] = &[
        "probe.obj",
        "probe.png",
        "probe.wav",
        "probe.wgsl",
        "probe.ttf",
        "probe.kscene",
        "probe.kmat",
        "probe.erg",
        "probe.kprefab",
    ];

    /// The asset types a loose-files project can decode. The SDK's
    /// `run_default` test pins the same list for the runtime's loose-files
    /// mode: the editor and a runtime reading the same project decode the same
    /// assets.
    const DECODABLE: &[&str] = &[
        "audio", "font", "material", "mesh", "prefab", "script", "shader", "texture",
    ];

    /// The asset types among [`PROBES`] that `svc` has a decoder for, sorted.
    fn decodable_types(svc: &mut AssetService) -> Result<Vec<String>> {
        let mut decodable = Vec::new();
        for file in PROBES {
            let uuid = AssetUUID::new_v5(file);
            let type_name = svc
                .index()
                .get_metadata(&uuid)
                .ok_or_else(|| anyhow::anyhow!("{file} is not indexed"))?
                .asset_type_name
                .clone();
            let no_decoder = match svc.load::<Mesh>(&uuid) {
                Ok(_) => false,
                Err(e) => format!("{e:#}").contains("No decoder registered"),
            };
            if !no_decoder {
                decodable.push(type_name);
            }
        }
        decodable.sort();
        Ok(decodable)
    }

    #[test]
    fn an_opened_project_registers_every_decoder() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-project-decoders-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets)?;
        for file in PROBES {
            std::fs::write(assets.join(file), b"not a real asset")?;
        }

        let decodable = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
            .and_then(|pvfs| decodable_types(&mut pvfs.assets()));
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(decodable?, DECODABLE);
        Ok(())
    }

    /// A renamed asset keeps the UUID its references stored: an opened project
    /// indexes it under the identity `.khora/asset-registry.ron` froze, not the
    /// one its new path would derive. The SDK's loose-files runtime is held to
    /// the same by its own test.
    #[test]
    fn an_opened_project_resolves_the_uuids_the_registry_froze() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-project-registry-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let scenes = root.join("assets").join("scenes");
        std::fs::create_dir_all(&scenes)?;
        std::fs::write(scenes.join("renamed.kscene"), b"scene bytes")?;

        let frozen = khora_sdk::khora_core::asset::AssetUUID::new_v5("scenes/original.kscene");
        let mut registry = khora_sdk::AssetIdRegistry::load(&root);
        registry.freeze("scenes/renamed.kscene", frozen);
        registry.save()?;

        let resolved =
            ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new())).map(|pvfs| {
                let by_frozen = pvfs.assets().load_raw(&frozen).ok();
                let by_path = pvfs
                    .assets()
                    .index()
                    .get_metadata(&khora_sdk::khora_core::asset::AssetUUID::new_v5(
                        "scenes/renamed.kscene",
                    ))
                    .is_some();
                (by_frozen, by_path)
            });
        let _ = std::fs::remove_dir_all(&root);
        let (by_frozen, by_path) = resolved?;

        assert_eq!(by_frozen.as_deref(), Some(&b"scene bytes"[..]));
        assert!(!by_path, "indexed under its path-derived UUID");
        Ok(())
    }

    /// A project directory under the temp folder, with an `assets/` folder,
    /// removed when the test ends — panicking or not.
    struct TempProject(PathBuf);

    impl TempProject {
        fn new(name: &str) -> Result<Self> {
            let root = std::env::temp_dir().join(format!(
                "khora-editor-create-asset-{name}-{}",
                std::process::id()
            ));
            if root.exists() {
                std::fs::remove_dir_all(&root)?;
            }
            std::fs::create_dir_all(root.join("assets"))?;
            Ok(Self(root))
        }

        fn open(&self) -> Result<ProjectVfs> {
            ProjectVfs::open(self.0.clone(), Arc::new(MetricsRegistry::new()))
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn create_asset_writes_the_file_and_its_folders_and_indexes_it() -> Result<()> {
        let project = TempProject::new("writes")?;
        let mut pvfs = project.open()?;

        pvfs.create_asset("scripts/ai/Player.erg", b"behavior Player { }")?;

        let written = project
            .0
            .join("assets")
            .join("scripts")
            .join("ai")
            .join("Player.erg");
        assert_eq!(std::fs::read(&written)?, b"behavior Player { }");
        let listed = crate::hot_reload::collect_asset_entries(&pvfs);
        assert!(
            listed
                .iter()
                .any(|entry| entry.source_path == "scripts/ai/Player.erg"),
            "the index does not list the new file: {listed:?}"
        );
        Ok(())
    }

    #[test]
    fn create_asset_refuses_an_existing_file_and_leaves_its_bytes() -> Result<()> {
        let project = TempProject::new("existing")?;
        let scripts = project.0.join("assets").join("scripts");
        std::fs::create_dir_all(&scripts)?;
        std::fs::write(scripts.join("Player.erg"), b"what the author wrote")?;
        let mut pvfs = project.open()?;

        let created = pvfs.create_asset("scripts/Player.erg", b"a template");

        assert!(created.is_err(), "an existing file is overwritten");
        assert_eq!(
            std::fs::read(scripts.join("Player.erg"))?,
            b"what the author wrote"
        );
        Ok(())
    }

    #[test]
    fn create_asset_refuses_a_path_that_is_empty_or_leaves_or_bends_assets() -> Result<()> {
        let project = TempProject::new("refused")?;
        let outside = std::env::temp_dir().join(format!(
            "khora-editor-create-asset-outside-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&outside);
        let absolute = outside.join("absolute.erg");
        let absolute_fwd = absolute.to_string_lossy().replace('\\', "/");
        let mut pvfs = project.open()?;

        let refused = [
            "",
            "../escape.erg",
            "a/../b.erg",
            "./here.erg",
            "a//b.erg",
            "a/",
            "/rooted.erg",
            absolute_fwd.as_str(),
        ]
        .map(|rel| (rel.to_owned(), pvfs.create_asset(rel, b"bytes").is_err()));
        let absolute_written = absolute.exists();
        let _ = std::fs::remove_dir_all(&outside);

        for (rel, was_refused) in &refused {
            assert!(was_refused, "`{rel}` was accepted");
        }
        assert!(
            !absolute_written,
            "{} was written by an absolute path",
            absolute.display()
        );
        let assets = project.0.join("assets");
        for written in [
            project.0.join("escape.erg"),
            assets.join("b.erg"),
            assets.join("a"),
            assets.join("here.erg"),
            assets.join("rooted.erg"),
        ] {
            assert!(
                !written.exists(),
                "{} was written by a refused path",
                written.display()
            );
        }
        Ok(())
    }

    /// The engine holds copies of what the previous project's service served;
    /// once another project's service moves in under the same uuids (a path
    /// both projects share), those copies are stale and the engine must hear
    /// of each one.
    #[test]
    fn sharing_another_projects_assets_announces_what_the_previous_one_served() -> Result<()> {
        const REL: &str = "meshes/a.obj";
        let first = TempProject::new("share-first")?;
        let second = TempProject::new("share-second")?;
        for (project, obj) in [
            (&first, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n"),
            (&second, "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n"),
        ] {
            let meshes = project.0.join("assets").join("meshes");
            std::fs::create_dir_all(&meshes)?;
            std::fs::write(meshes.join("a.obj"), obj)?;
        }
        let mesh = AssetUUID::new_v5(REL);
        let shared = Arc::new(Mutex::new(AssetService::empty()));

        let mut first_vfs = first.open()?;
        first_vfs.share_assets(&shared);
        first_vfs.assets().load::<Mesh>(&mesh)?;

        let mut second_vfs = second.open()?;
        second_vfs.share_assets(&shared);

        let announced: Vec<AssetUUID> = shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .changes()
            .drain()
            .into_iter()
            .map(|change| change.0)
            .collect();
        assert!(
            announced.contains(&mesh),
            "the mesh the first project served is announced: {announced:?}"
        );
        Ok(())
    }

    /// A glTF whose one scene shows node `Hull`: a triangle in the default
    /// material.
    const ONE_NODE_GLTF: &str = r#"{
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "Hull", "mesh": 0, "translation": [3.0, 0.0, 0.0] }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 } }] }],
        "buffers": [{
            "byteLength": 36,
            "uri": "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"
        }],
        "bufferViews": [{ "buffer": 0, "byteLength": 36 }],
        "accessors": [{
            "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
            "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
        }]
    }"#;

    /// The editor's prefab source reads a glTF scene sub-asset as the scene's
    /// record: its root, then the node, showing the file's sub-assets.
    #[test]
    fn a_gltf_scene_is_a_prefab_record_of_the_project() -> Result<()> {
        use khora_sdk::khora_data::ecs::{MeshRef, Name};

        let root = std::env::temp_dir().join(format!(
            "khora-editor-project-gltf-prefab-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let models = root.join("assets").join("models");
        std::fs::create_dir_all(&models)?;
        std::fs::write(models.join("ship.gltf"), ONE_NODE_GLTF)?;

        let file = AssetUUID::new_v5("models/ship.gltf");
        let scene = AssetUUID::sub_asset(file, "scene");
        let record = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
            .map(|mut pvfs| pvfs.prefab_record(scene));
        let _ = std::fs::remove_dir_all(&root);
        let record = record?.map_err(|e| anyhow::anyhow!("the scene reads: {e}"))?;

        let mut world = khora_sdk::khora_data::ecs::World::new();
        khora_sdk::khora_data::scene::apply(
            &mut world,
            &record,
            khora_sdk::khora_data::scene::Identity::Keep,
        )
        .map_err(|e| anyhow::anyhow!("the record applies: {e}"))?;
        assert_eq!(record.entities.len(), 2, "the root and Hull");
        let hull = world
            .iter_entities()
            .find(|e| world.get::<Name>(*e).is_some_and(|n| n.as_str() == "Hull"))
            .expect("Hull is in the record");
        assert_eq!(
            world.get::<MeshRef>(hull).cloned(),
            Some(MeshRef::Asset(AssetUUID::sub_asset(file, "mesh0/prim0")))
        );
        Ok(())
    }

    fn reveal(rel: &str) -> AfterIndex {
        AfterIndex::Reveal(rel.to_owned())
    }

    /// **Without a hub, a write reindexes at once.** A project opened with
    /// no engine (a test, an editor with no engine) has no index hub:
    /// `reindex` rebuilds the index on the frame, so the file is indexed
    /// when it returns, and an action asked for after it runs now.
    #[test]
    fn without_a_hub_writes_reindex_at_once() -> Result<()> {
        const PLAYER: &str = "scripts/Player.erg";
        let project = TempProject::new("no-hub-reindex")?;
        let mut pvfs = project.open()?;
        pvfs.write_asset(Path::new(PLAYER), b"behavior Player { }")?;
        let player = pvfs.resolve_uuid(PLAYER);
        assert!(
            pvfs.assets().index().get_metadata(&player).is_none(),
            "the premise: a write alone does not index"
        );
        let before = pvfs.generation();

        pvfs.reindex()?;

        assert!(
            pvfs.assets().index().get_metadata(&player).is_some(),
            "the file is indexed when `reindex` returns"
        );
        assert!(pvfs.generation() > before, "the project's generation moved");
        assert_eq!(
            pvfs.after_index(reveal(PLAYER)),
            Some(reveal(PLAYER)),
            "nothing is in flight: the action runs now"
        );
        assert!(pvfs.land().is_none(), "nothing was filed, nothing lands");
        Ok(())
    }

    /// **With a hub and nothing in flight, an action runs now.** No write
    /// waits to be indexed: `after_index` hands the action back.
    #[test]
    fn with_a_hub_and_nothing_in_flight_an_action_runs_now() {
        use crate::tests::gltf_fixture::GltfProject;
        use crate::tests::project_index::IndexingEngine;

        let engine = IndexingEngine::new();
        let project = GltfProject::with("hub-nothing-in-flight", &[]);
        engine.serve(&project.vfs);
        let mut pvfs = project.vfs.lock().expect("the project");

        assert_eq!(
            pvfs.after_index(reveal("scripts/Player.erg")),
            Some(reveal("scripts/Player.erg"))
        );
        assert!(pvfs.land().is_none(), "nothing was filed, nothing lands");
        assert_eq!(engine.filed(), 0, "no reindex was filed");
    }

    /// **A write during a reindex waits for the next.** A first write files
    /// a reindex, and its project walk is built; a second write arrives
    /// before that reindex lands. At its landing, only the first write's
    /// action runs, and the index does not hold the second file — its walk
    /// began before it. A second reindex is filed then, not before: one in
    /// flight at a time. At its landing, the second write's action runs and
    /// the index holds the second file. Each landing moves the project's
    /// generation.
    #[test]
    fn a_write_during_a_reindex_waits_for_the_next() {
        use crate::tests::gltf_fixture::GltfProject;
        use crate::tests::project_index::IndexingEngine;

        const FIRST: &str = "scripts/First.erg";
        const SECOND: &str = "scripts/Second.erg";
        let engine = IndexingEngine::new();
        let project = GltfProject::with("write-during-reindex", &[]);
        engine.serve(&project.vfs);
        let (first, second) = (project.uuid(FIRST), project.uuid(SECOND));
        let mut pvfs = project.vfs.lock().expect("the project");

        pvfs.create_asset(FIRST, b"behavior First { }")
            .expect("the first file is created");
        assert_eq!(
            pvfs.after_index(reveal(FIRST)),
            None,
            "the first action waits for the reindex in flight"
        );
        assert_eq!(engine.waiting(), 1, "the first write filed a reindex");
        // The first reindex walks the project now, before the second write.
        engine.build();

        pvfs.create_asset(SECOND, b"behavior Second { }")
            .expect("the second file is created");
        assert_eq!(
            pvfs.after_index(reveal(SECOND)),
            None,
            "the second action waits for the next reindex"
        );
        assert_eq!(
            engine.waiting(),
            0,
            "no second reindex while the first is in flight"
        );
        assert!(pvfs.land().is_none(), "the first is still in flight");

        engine.intake();
        let generation = pvfs.generation();
        let landed = pvfs.land().expect("the first reindex landed");
        assert_eq!(landed.outcome, Ok(()));
        assert_eq!(
            landed.actions,
            vec![reveal(FIRST)],
            "only the first write's action runs at the first landing"
        );
        assert!(
            pvfs.generation() > generation,
            "the landing moves the generation"
        );
        assert!(engine.indexes(&first), "the first file is indexed");
        assert!(
            !engine.indexes(&second),
            "the second file is not: the walk began before it"
        );
        assert_eq!(
            engine.waiting(),
            1,
            "the second reindex is filed at the first's landing"
        );
        assert!(pvfs.land().is_none(), "the second is in flight");

        engine.frame();
        let generation = pvfs.generation();
        let landed = pvfs.land().expect("the second reindex landed");
        assert_eq!(landed.outcome, Ok(()));
        assert_eq!(
            landed.actions,
            vec![reveal(SECOND)],
            "the second write's action runs at the second landing"
        );
        assert!(
            pvfs.generation() > generation,
            "the landing moves the generation"
        );
        assert!(engine.indexes(&second), "the second file is indexed");

        assert!(pvfs.land().is_none(), "nothing more lands");
        assert_eq!(engine.waiting(), 0, "nothing more is filed");
        assert_eq!(engine.filed(), 2, "two reindexes in all");
        assert_eq!(engine.most_waiting(), 1, "one reindex in flight at a time");
    }

    /// **A project opened over the engine's service is not walked.** Its
    /// `assets/` holds a file, but the engine's service is empty: the
    /// project serves that very service, so it indexes nothing — the index
    /// comes through the engine's `IndexHub`. The identity registry's file is
    /// made and the project's assets are watched, as `open_watched` does.
    #[test]
    fn a_project_opened_over_the_engines_service_is_not_walked() -> Result<()> {
        let project = TempProject::new("over-service-not-walked")?;
        let textures = project.0.join("assets").join("textures");
        std::fs::create_dir_all(&textures)?;
        std::fs::write(textures.join("crate.png"), b"PNG")?;
        let shared = Arc::new(Mutex::new(AssetService::empty()));
        let watch = Arc::new(AssetWatch::default());

        let pvfs = ProjectVfs::over_service(project.0.clone(), Arc::clone(&shared), watch.clone())?;

        assert!(
            Arc::ptr_eq(&pvfs.asset_service, &shared),
            "the project serves the engine's service, not one of its own"
        );
        assert_eq!(
            pvfs.assets().index().asset_count(),
            0,
            "the project's files were walked on the frame"
        );
        assert!(
            pvfs.assets()
                .index()
                .get_metadata(&AssetUUID::new_v5("textures/crate.png"))
                .is_none(),
            "the file is indexed before any `Open` landed"
        );
        assert!(
            project
                .0
                .join(".khora")
                .join("asset-registry.ron")
                .is_file(),
            "the identity registry's file is not made"
        );
        let assets = std::fs::canonicalize(project.0.join("assets"))?;
        assert!(
            watch
                .roots()
                .iter()
                .filter_map(|root| std::fs::canonicalize(root).ok())
                .any(|root| root == assets),
            "the project's assets are not watched: {:?}",
            watch.roots()
        );
        Ok(())
    }

    /// **A project opened over the engine's service gets its `assets/`.** A
    /// project folder holding no `assets/` yet opens with one, empty: the
    /// writer and the watch have a folder to work in.
    #[test]
    fn a_project_opened_over_the_engines_service_gets_its_assets_folder() -> Result<()> {
        let project = TempProject::new("over-service-assets-folder")?;
        std::fs::remove_dir_all(project.0.join("assets"))?;
        let shared = Arc::new(Mutex::new(AssetService::empty()));

        let pvfs =
            ProjectVfs::over_service(project.0.clone(), shared, Arc::new(AssetWatch::default()))?;

        assert!(
            project.0.join("assets").is_dir(),
            "the project's `assets/` is not created"
        );
        assert_eq!(pvfs.assets_root, project.0.join("assets"));
        Ok(())
    }

    /// **The project's physics step is read from its `project.json`.** A
    /// `physics` section's step is the project's; a project with no section,
    /// with no `project.json`, or with a section that does not read (a zero
    /// step) runs at the default step.
    #[test]
    fn the_physics_settings_are_the_physics_section_of_project_json() -> Result<()> {
        let a_128th = std::time::Duration::from_nanos(7_812_500);
        let project = TempProject::new("physics-settings")?;
        let read = |json: Option<&str>| -> Result<PhysicsSettings> {
            let path = project.0.join("project.json");
            match json {
                Some(json) => std::fs::write(&path, json)?,
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
            Ok(project.open()?.physics_settings())
        };

        assert_eq!(
            read(Some(r#"{ "name": "p", "physics": { "step": 0.0078125 } }"#))?.step,
            a_128th,
            "the section's step is the project's"
        );
        assert_eq!(
            read(Some(r#"{ "name": "p" }"#))?,
            PhysicsSettings::default()
        );
        assert_eq!(read(None)?, PhysicsSettings::default());
        assert_eq!(
            read(Some(r#"{ "name": "p", "physics": { "step": 0 } }"#))?,
            PhysicsSettings::default(),
            "a step that does not read leaves the default"
        );
        Ok(())
    }
}
