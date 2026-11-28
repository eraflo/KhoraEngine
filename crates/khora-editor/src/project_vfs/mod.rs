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

//! The editor's project-scoped virtual file system: every read and write of
//! project content goes through it.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::{Context, Result};
use khora_sdk::khora_core::asset::{asset_key, AssetUUID};
use khora_sdk::khora_core::physics::PhysicsSettings;
use khora_sdk::khora_core::sync::LockExt;
#[cfg(test)]
use khora_sdk::MetricsRegistry;
use khora_sdk::{
    AssetChangeEvent, AssetIdRegistry, AssetService, AssetWatch, AssetWriter, FileLoader,
    IndexBuilder,
};

mod asset_files;
mod asset_path;
mod in_project;
mod reached;
mod write_slot;

pub use crate::after_index::{AfterIndex, Landed};
pub use crate::project_reads::ProjectReads;
pub use asset_path::{check_existing, check_new_name, new_name_from};
pub use in_project::InProject;
pub use khora_sdk::editor_ui::KeyChange;
pub use write_slot::WriteSlot;

/// All project I/O, in one place.
///
/// Assets are indexed under `<root>/assets/` by identity: `AssetUUID::new_v5`
/// of the forward-slash path, unless the identity registry froze another, so
/// a pack built from the project names every asset the same.
pub struct ProjectVfs {
    pub root: PathBuf,
    pub assets_root: PathBuf,
    /// The project's assets, shared with the engine ([`Self::share_assets`]):
    /// the editor and the frame read one index and one cache.
    asset_service: Arc<Mutex<AssetService>>,
    /// Where its changes are read from: the engine's shared watch, or one of
    /// its own when the project was opened alone.
    watch: Arc<AssetWatch>,
    file_loader: FileLoader,
    /// Stable-identity registry (`<root>/.khora/asset-registry.ron`), written by the editor only.
    ///
    /// File operations freeze, rename and remove its entries, so an asset
    /// keeps its UUID across renames and every reference keeps resolving.
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
    /// The assets root as named and resolved, resolved once: what a change's
    /// root is matched against.
    watched_as: khora_sdk::fs::RootName,
    /// What the project's files are read through ([`Self::over_service`]).
    reads: ProjectReads,
}

impl ProjectVfs {
    /// Opens (or creates) `<root>/assets/`, builds the VFS and arms a watcher of its own.
    ///
    /// A fresh project with an empty `assets/` opens with nothing indexed.
    #[cfg(test)]
    pub fn open(root: PathBuf, metrics: Arc<MetricsRegistry>) -> Result<Self> {
        Self::open_watched(root, metrics, Arc::new(AssetWatch::default()))
    }

    /// Opens `<root>/assets/` as [`Self::open`] does, reading its changes from the shared `watch`.
    ///
    /// [`Self::poll_changes`] then reports the changes under this project's
    /// assets root only. For tests: a run opens a project through the
    /// engine's index (`start_opening_project`), at the trust its user decided.
    #[cfg(test)]
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
        let mut registry = AssetIdRegistry::load_or_unread(&ProjectReads::default().guard(&root));
        // Made on first open, so the file is visible in the project even
        // before the first rename.
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

        let mut opened = Self {
            root,
            watched_as: khora_sdk::fs::RootName::of(&assets_root),
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
            reads: ProjectReads::default(),
        };
        // The index just built may hold derived UUIDs (a file made at a renamed
        // path): they are frozen as at a reindex.
        opened.freeze_derived_ids();
        Ok(opened)
    }

    /// Opens the project at `root` over `service`, the engine's shared asset service.
    ///
    /// The service was filled off the frame by an `Open` filed with the
    /// engine's index hub, so nothing is walked. As `open_watched` (tests)
    /// does, it makes `<root>/assets/` and the identity registry's file, and
    /// arms `watch`. Its files are read through `reads`, the registry's first.
    pub fn over_service(
        root: PathBuf,
        service: Arc<Mutex<AssetService>>,
        watch: Arc<AssetWatch>,
        reads: ProjectReads,
    ) -> Result<Self> {
        let assets_root = root.join("assets");
        std::fs::create_dir_all(&assets_root).with_context(|| {
            format!(
                "Failed to ensure project assets directory exists: {}",
                assets_root.display()
            )
        })?;
        // The identity registry, made on first open, as `open_watched` does.
        // One refused is reported, and never written over.
        let mut registry = AssetIdRegistry::load_or_unread(&reads.guard(&root));
        if let Err(e) = registry.ensure_file() {
            log::warn!("Could not create asset identity registry file: {e}");
        }
        let file_loader = FileLoader::new(&assets_root);
        watch
            .watch(&assets_root)
            .context("Failed to watch the project's assets (filesystem hot reload)")?;
        let mut opened = Self {
            root,
            watched_as: khora_sdk::fs::RootName::of(&assets_root),
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
            reads,
        };
        // What was indexed while the editor was closed is frozen as at a reindex.
        opened.freeze_derived_ids();
        Ok(opened)
    }

    /// Locks the project's asset service for the length of a statement.
    pub fn assets(&self) -> MutexGuard<'_, AssetService> {
        self.asset_service.lock_recovered()
    }

    /// Serves this project's assets through `shared`, the engine's service, from now on.
    ///
    /// The engine then resolves the editor's entities' meshes, materials and
    /// textures from the index and cache the editor reads.
    pub fn share_assets(&mut self, shared: &Arc<Mutex<AssetService>>) {
        if Arc::ptr_eq(&self.asset_service, shared) {
            return;
        }
        {
            let mut own = self.assets();
            let mut target = shared.lock_recovered();
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

    /// Returns what the project's files are read through.
    pub fn reads(&self) -> &ProjectReads {
        &self.reads
    }

    /// Walks `<root>/assets/` again and swaps the VFS index, on the frame.
    ///
    /// What [`Self::reindex`] does without an engine, and what the setup-time
    /// scene load does. The files it looks into are read through
    /// [`Self::reads`], at the open project's trust, under the identities the
    /// registry's file holds now (it is followed first, as the engine reads it
    /// at every reindex). Freezes the derived UUIDs it now lists, saving the
    /// registry if any.
    pub fn rebuild_index(&mut self) -> Result<()> {
        self.follow_registry();
        let bytes = IndexBuilder::new(self.reads.guard(&self.assets_root))
            .with_registry(&self.registry)
            .build_index_bytes()
            .context("Failed to rebuild project asset index")?;
        self.assets().reindex(&bytes)?;
        self.freeze_derived_ids();
        if let Some(catalog) = &self.prefab_catalog {
            catalog.replace(khora_sdk::prefab_catalog(&self.assets()));
        }
        self.moved_on();
        Ok(())
    }

    /// Puts back under its name each entry under `assets/` a rename cut short left aside; returns what it did with each.
    ///
    /// For a project's open. An entry whose name something else holds now is
    /// left where it is, never removed. Indexes the project again when one
    /// was put back, a reindex that fails logged. See
    /// [`khora_sdk::fs::reclaim_asides`].
    pub fn reclaim_asides(&mut self) -> Vec<khora_sdk::fs::Reclaimed> {
        let reclaimed = khora_sdk::fs::reclaim_asides(&self.assets_root);
        let put_back = (reclaimed.iter())
            .any(|done| matches!(done, khora_sdk::fs::Reclaimed::Restored { .. }));
        if put_back {
            if let Err(error) = self.reindex() {
                log::error!("The project is not indexed again after a rename's leftover was put back: {error:#}");
            }
        }
        reclaimed
    }

    /// Takes in a reindex that landed in the engine's asset service.
    ///
    /// Refills the prefab catalog from the new index and moves the project's generation.
    /// Follows the registry's file, which the engine read to build the index,
    /// then freezes the derived UUIDs it now lists, saving the registry if any.
    pub(crate) fn index_landed(&mut self) {
        self.freeze_derived_ids();
        if let Some(catalog) = &self.prefab_catalog {
            catalog.replace(khora_sdk::prefab_catalog(&self.assets()));
        }
        self.moved_on();
    }

    /// Freezes the UUID of each file indexed whose default another path holds, and saves the registry.
    ///
    /// Such a UUID is derived (`AssetIdRegistry::resolve`): frozen, it stays
    /// the file's once the path holding the default is gone. The registry's
    /// file is followed first; a registry unread freezes nothing, as it could
    /// not keep it.
    fn freeze_derived_ids(&mut self) {
        self.follow_registry();
        if !self.registry.is_read() {
            return;
        }
        let paths: Vec<String> = (self.assets().index().iter_all())
            .map(|metadata| khora_sdk::khora_core::asset::asset_key(&metadata.source_path))
            .collect();
        if self
            .registry
            .freeze_derived(paths.iter().map(String::as_str))
        {
            if let Err(e) = self.registry.save() {
                log::error!("Failed to persist asset registry after indexing: {e}");
            }
        }
    }

    /// Reads the identity registry's file again when it changed since the registry read or wrote it.
    ///
    /// The file on disk is the truth: a pull or a merge can change it
    /// while the project is open. What it changed moves the project's
    /// generation; a file that does not parse leaves the registry unread
    /// (refused, reported), and what would change an identity is refused.
    pub(crate) fn follow_registry(&mut self) {
        if self.registry.follow() {
            self.moved_on();
        }
    }

    /// Files this project's reindexes with `hub`, the engine's index hub, from now on.
    ///
    /// They are built off the frame and land at a frame's boundary.
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

    /// Returns the actions kept until the edited scene was back.
    pub(crate) fn release(&mut self) -> Vec<AfterIndex> {
        std::mem::take(&mut self.held)
    }

    /// Returns the reindex in flight, if any.
    pub fn reindexing(&self) -> Option<khora_sdk::IndexTicket> {
        self.awaiting.as_ref().map(|(ticket, _)| *ticket)
    }

    /// Indexes the project again after a write.
    ///
    /// With a hub, a reindex is filed, or, while one is in flight, the next
    /// is filed once it lands; without a hub, the index is rebuilt on the frame.
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

    /// Makes the actions waiting for a reindex, or for the edited scene, follow `change`.
    ///
    /// One naming a key moved names its new key; one naming a key or an asset
    /// trashed is dropped ([`AfterIndex::followed`]).
    pub(crate) fn follow_key_change(&mut self, change: &KeyChange) {
        let follow = |actions: &mut Vec<AfterIndex>| {
            *actions = std::mem::take(actions)
                .into_iter()
                .filter_map(|action| action.followed(change))
                .collect();
        };
        if let Some((_, waiting)) = &mut self.awaiting {
            follow(waiting);
        }
        if let Some(next) = &mut self.next {
            follow(next);
        }
        follow(&mut self.held);
    }

    /// Takes the outcome of the reindex in flight, once it landed.
    ///
    /// On `Ok` the project takes it in; the next reindex is filed if one was
    /// asked for. `None` while it is in flight, or when none is. The actions
    /// that waited for a failed reindex are dropped: what they needed is not
    /// in the index.
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

    /// Returns the project's physics settings, from its `project.json`.
    ///
    /// The defaults when it has no `physics` section; a section that does not
    /// read (a zero or negative step) is reported and the defaults kept.
    pub fn physics_settings(&self) -> PhysicsSettings {
        crate::project_physics::project_physics(&self.reads.guard(&self.root))
    }

    /// Writes `bytes` to `<root>/assets/<rel_path>`, creating its folders, for a caller that needs no asset id.
    ///
    /// The file lands where [`Self::reserve_write`] would land it: a file
    /// written over, however `rel_path` spells it where file names ignore case,
    /// is the same asset, and keeps its identity, the registry read or not; a
    /// new file is a new asset, the entry a trashed file left at its path
    /// dropped first, saving the registry. A caller that needs the id the file
    /// holds (it names itself, or is assigned) reserves the write instead. Not
    /// indexed: call [`Self::rebuild_index`] for the path to resolve through
    /// the VFS in the same frame.
    ///
    /// # Errors
    ///
    /// When the write cannot land at `rel_path`, as for
    /// [`Self::reserve_write`]; when no file is there and the identity
    /// registry was not read, or cannot be saved without a trashed file's
    /// entry at the path (nothing written); when the file cannot be written.
    pub fn write_asset(&mut self, rel_path: &Path, bytes: &[u8]) -> Result<()> {
        self.follow_registry();
        let (rel, _) = self.landing(&asset_key(rel_path))?;
        let freed = self.free_path_for_new_file(&rel, &format!("Saving a new file at '{rel}'"))?;
        let written = self
            .file_loader
            .write_bytes(Path::new(&rel), bytes)
            .map(|()| self.moved_on());
        self.give_back_on_failure(&rel, freed, written)
    }

    /// Returns whether the file at `path` is one of the project's assets, and its forward-slash path under `assets/` when it is.
    ///
    /// The one answer to whether a file picked by the user is the project's.
    /// `path` is resolved as far as it exists, links followed. The parts that
    /// exist are named as spelled on disk, whatever case `path` uses where
    /// file names ignore it; the parts below, not made yet, as typed, never
    /// trimmed. A path named under the assets is refused when it leads out of
    /// them through a link, when a part that exists, as typed or as resolved,
    /// is one the system does not reach under that name ([`check_existing`]),
    /// or when a new part is no name a project keeps ([`check_new_name`]). A
    /// file inside is written through [`Self::write_asset`].
    pub fn in_project(&self, path: &Path) -> InProject {
        let Some(named) = khora_sdk::fs::relative_to(path, &self.assets_root) else {
            return InProject::Outside;
        };
        if named.as_os_str().is_empty() {
            return InProject::Outside;
        }
        // As typed first: a name the system trims or reads as a device would
        // resolve onto another entry, and be filed under that entry's name.
        if let Err(predicate) = self.named_part_by_part(&named) {
            return InProject::Refused(format!("'{}' {predicate}", path.display()));
        }
        let Some(resolved) = khora_sdk::fs::resolved_relative_to(path, &self.watched_as.resolved)
        else {
            return InProject::Refused(format!(
                "'{}' leads out of the project's assets through a link",
                path.display()
            ));
        };
        match self.named_part_by_part(&resolved) {
            Ok(rel) if !rel.is_empty() => InProject::Inside(rel),
            Ok(_) => InProject::Outside,
            Err(predicate) => InProject::Refused(format!("'{}' {predicate}", path.display())),
        }
    }

    /// Returns the forward-slash path under `assets/` of the file at `path`, as [`Self::in_project`] names it; `None` when not inside.
    #[cfg(test)]
    pub(crate) fn rel_inside_assets(&self, path: &Path) -> Option<String> {
        match self.in_project(path) {
            InProject::Inside(rel) => Some(rel),
            InProject::Outside | InProject::Refused(_) => None,
        }
    }

    /// Resolves a forward-slash relative path to its UUID through the identity registry.
    ///
    /// A renamed asset keeps its frozen identity; any other gets its `new_v5`
    /// default, or a derived UUID when another path holds that default.
    /// Use it whenever an asset the user could have renamed is looked up by path.
    pub fn resolve_uuid(&self, rel_path_fwd_slash: &str) -> AssetUUID {
        self.registry.resolve(rel_path_fwd_slash)
    }

    /// Returns whether the index lists the asset at `rel_path_fwd_slash` and its file is on disk.
    pub(crate) fn indexed_and_present(&self, rel_path_fwd_slash: &str) -> bool {
        let uuid = self.resolve_uuid(rel_path_fwd_slash);
        self.assets().index().get_metadata(&uuid).is_some()
            && self.abs_of(rel_path_fwd_slash).is_file()
    }

    /// Returns the absolute path of the asset at forward-slash path `rel_fwd`.
    fn abs_of(&self, rel_fwd: &str) -> PathBuf {
        let mut p = self.assets_root.clone();
        for seg in rel_fwd.split('/').filter(|s| !s.is_empty()) {
            p.push(seg);
        }
        p
    }

    /// Returns `rel`, a path under `assets/`, forward-slash: the parts that exist as spelled on disk, the rest as given.
    ///
    /// Each part is opened as [`Self::abs_of`] names it, under the assets
    /// root as named: a part that exists must be reached under that name, the
    /// rest are held to a new name's rules ([`asset_path::named_from`]).
    ///
    /// # Errors
    ///
    /// The predicate saying why a part is refused, naming it: a link that
    /// leads nowhere, a part that exists and that the system does not reach
    /// under that name, or a new part that is no name a project keeps.
    fn named_part_by_part(&self, rel: &Path) -> std::result::Result<String, String> {
        let parts: Vec<String> = (rel.components())
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        asset_path::named_from(
            &self.assets_root,
            parts.iter().map(String::as_str),
            asset_path::not_portable,
        )
    }

    /// Reads the identity registry again through [`Self::reads`], at the project's trust as it is now.
    ///
    /// For a trust change: a registry refused at the former trust reads at the
    /// new one, and the operations an unread registry refuses work again.
    /// Freezes the derived UUIDs of the index, as an open does, and moves the
    /// project's generation.
    pub(crate) fn reread_at_its_trust(&mut self) {
        self.registry = AssetIdRegistry::load_or_unread(&self.reads.guard(&self.root));
        self.freeze_derived_ids();
        self.moved_on();
    }

    /// Drains the pending hot-reload events under this project's assets root.
    pub fn poll_changes(&self) -> Vec<AssetChangeEvent> {
        let mut events = self.watch.poll_for("editor_vfs");
        events.retain(|event| self.watched_as.names(&event.root));
        events
    }
}

impl ProjectVfs {
    /// Returns the asset id a write to `rel_path_fwd_slash` gives, as [`Self::reserve_write`] reserves it, reserving nothing.
    ///
    /// The registry's file is followed first. For tests: an editor write takes
    /// its id from its reservation.
    #[cfg(test)]
    pub(crate) fn written_asset_id(&mut self, rel_path_fwd_slash: &str) -> AssetUUID {
        self.follow_registry();
        self.landing(rel_path_fwd_slash).map_or_else(
            |_| self.resolve_uuid(rel_path_fwd_slash),
            |(rel, new_file)| self.slot_at(rel, new_file).id(),
        )
    }

    /// Returns the file of the asset known as `id`, relative to `assets/`, as
    /// the index knows it.
    pub fn rel_path_of(&self, id: AssetUUID) -> Option<String> {
        let assets = self.assets();
        let source = &assets.index().get_metadata(&id)?.source_path;
        let rel = source.strip_prefix(&self.assets_root).unwrap_or(source);
        Some(asset_key(rel))
    }

    /// Returns whether `id` names a part of a file (a glTF's scene, mesh, material…).
    ///
    /// Such a part shares its file's path and is never written.
    pub(crate) fn is_part_of_a_file(&self, id: AssetUUID) -> bool {
        self.assets()
            .index()
            .get_metadata(&id)
            .and_then(|metadata| metadata.default_source())
            .is_some_and(|source| matches!(source, khora_sdk::AssetSource::Sub { .. }))
    }

    /// Reads the record of the prefab known as `id`.
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
mod tests;
