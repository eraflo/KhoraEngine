// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Project hot-reload pump.
//!
//! Drains pending filesystem-change events from the project watcher,
//! invalidates the `AssetService` cache for modified UUIDs, and reindexes
//! when files are added or removed. Run once per frame, before the agents
//! see the world, so a coherent VFS is in scope for the rest of the tick.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::AssetEntry;
use khora_sdk::AssetChangeKind;
use khora_sdk::EditorState;

use crate::project_vfs::ProjectVfs;

/// Drain the project's filesystem watcher and apply the appropriate
/// invalidation / reindex actions. The asset browser cache is rebuilt
/// whenever a full reindex was queued.
pub fn pump(pvfs_mutex: &Arc<Mutex<ProjectVfs>>, editor_state: &Arc<Mutex<EditorState>>) {
    let Ok(mut pvfs) = pvfs_mutex.lock() else {
        return;
    };
    let events = pvfs.poll_changes();
    if events.is_empty() {
        return;
    }

    // Coalesce Modified by path (one save fires a flurry); Created/Removed
    // trigger a full reindex.
    let mut needs_reindex = false;
    let mut modified: HashSet<String> = HashSet::new();
    for e in events {
        match e.kind {
            AssetChangeKind::Modified => {
                modified.insert(e.rel_path);
            }
            AssetChangeKind::Created | AssetChangeKind::Removed => {
                log::info!(
                    "Hot reload: {:?} '{}' — full reindex queued",
                    e.kind,
                    e.rel_path
                );
                needs_reindex = true;
            }
        }
    }

    // Resolve the UUID through the identity registry (not the event's raw
    // path-derived value) so a modified *renamed* asset invalidates the right
    // cache entry.
    for rel in &modified {
        let uuid = pvfs.resolve_uuid(rel);
        let dropped = pvfs.asset_service.invalidate(&uuid);
        log::info!(
            "Hot reload: Modified '{}' (cache dropped: {})",
            rel,
            dropped
        );
    }

    if !needs_reindex {
        return;
    }
    if let Err(e) = pvfs.rebuild_index() {
        log::error!("Hot reload: failed to rebuild index: {:#}", e);
        return;
    }
    let entries = collect_asset_entries(&pvfs);
    let dirs = pvfs.list_dirs();
    if let Ok(mut state) = editor_state.lock() {
        state.asset_entries = entries;
        state.asset_dirs = dirs;
        state.asset_epoch = state.asset_epoch.wrapping_add(1);
    }
}

/// Build an `AssetEntry` cache from the current VFS contents. Used by the
/// initial project open and when the user browses to a different folder
/// at runtime.
pub fn collect_asset_entries(pvfs: &ProjectVfs) -> Vec<AssetEntry> {
    pvfs.asset_service
        .index()
        .iter_all()
        .map(|m| {
            // Forward slashes whatever the platform: the browser's folders,
            // the identity registry and the project VFS all name an asset so.
            let rel_str = m.source_path.to_string_lossy().replace('\\', "/");
            let name = m
                .source_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_str.clone());
            AssetEntry {
                name,
                asset_type: m.asset_type_name.clone(),
                source_path: rel_str,
                uuid: m.uuid,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use khora_sdk::khora_core::asset::AssetUUID;
    use khora_sdk::{AssetIdRegistry, MetricsRegistry};

    use super::collect_asset_entries;
    use crate::project_vfs::ProjectVfs;

    /// **Each entry carries the identity the index gave its asset** — the one
    /// the registry froze for a renamed file, the path's own for the others:
    /// what a reference to the asset stores.
    #[test]
    fn each_asset_entry_carries_the_identity_the_index_resolved() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-asset-entry-uuid-{}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        let prefabs = root.join("assets").join("prefabs");
        std::fs::create_dir_all(&prefabs)?;
        std::fs::write(prefabs.join("bullet.kprefab"), b"prefab bytes")?;
        std::fs::write(prefabs.join("rocket.kprefab"), b"prefab bytes")?;

        // `rocket` was renamed from `missile`: it keeps the identity it had.
        let frozen = AssetUUID::new_v5("prefabs/missile.kprefab");
        let mut registry = AssetIdRegistry::load(&root);
        registry.freeze("prefabs/rocket.kprefab", frozen);
        registry.save()?;

        let entries = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
            .map(|pvfs| collect_asset_entries(&pvfs));
        let _ = std::fs::remove_dir_all(&root);
        let entries = entries?;

        let uuid_of = |path: &str| {
            entries
                .iter()
                .find(|entry| entry.source_path.replace('\\', "/") == path)
                .map(|entry| entry.uuid)
        };
        assert_eq!(
            uuid_of("prefabs/bullet.kprefab"),
            Some(AssetUUID::new_v5("prefabs/bullet.kprefab")),
            "an asset never renamed is named by its path: {entries:?}"
        );
        assert_eq!(
            uuid_of("prefabs/rocket.kprefab"),
            Some(frozen),
            "a renamed asset keeps the identity the registry froze: {entries:?}"
        );
        Ok(())
    }
}
