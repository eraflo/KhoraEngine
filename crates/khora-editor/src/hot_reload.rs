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
        .vfs()
        .iter_all()
        .map(|m| {
            let rel_str = m.source_path.to_string_lossy().to_string();
            let name = m
                .source_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_str.clone());
            AssetEntry {
                name,
                asset_type: m.asset_type_name.clone(),
                source_path: rel_str,
            }
        })
        .collect()
}
