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

//! Pointing the engine at a project's scripts.
//!
//! The queues live in the engine bootstrap, for every application. What only
//! the application knows is **where the scripts are**: a game watches the
//! `assets/` beside its executable, the editor watches whichever project is
//! open, and a packed runtime watches nothing at all.
//!
//! This used to live inside `run_default`, which one binary calls. The editor
//! and the sandbox therefore ran the scripting agent and its lane over a
//! program table nothing could fill.

use std::path::Path;
use std::sync::Arc;

use khora_core::Runtime;
use khora_io::asset::AssetWatcher;

/// Watches `assets_root` and compiles the scripts already in it.
///
/// Returns how many modules compiled. Safe to call with a directory that does
/// not exist — a project without scripts is not an error, and neither is a
/// packed build with no source tree.
///
/// Two things happen here, and they are different:
///
/// - **The initial load.** The hot-reload pump reacts to *changes*, so without
///   this a game starts with no programs and its scripts only begin working
///   once someone saves a file. It takes the same road a reload does, so a
///   program reaches the runtime one way rather than two.
/// - **The watcher**, and only if the slot is free. An `AssetWatcher` derives an
///   asset's identity from its path relative to **one** root, so a runtime holds
///   exactly one. An application that already registered one — the sandbox
///   watches the engine's shader tree — keeps it, and its scripts load without
///   hot-reload until the multi-root watcher lands.
pub fn mount(runtime: &mut Runtime, assets_root: &Path) -> usize {
    if !assets_root.is_dir() {
        log::debug!(
            "scripts::mount: {} is not a directory — no scripts, no watcher",
            assets_root.display()
        );
        return 0;
    }

    // The watcher only if nobody claimed the slot. An `AssetWatcher` translates
    // a path against **one** root to derive an asset's identity, so there is
    // exactly one per runtime today — the sandbox spends its on the engine's
    // shader tree, which is a legitimate use.
    //
    // The initial load below does not need it, and that split matters: it is
    // what makes a project's scripts *run* everywhere, while hot-reloading them
    // waits on the multi-root watcher (`docs/plans/2026-08-07_engine-owned-hot-reload.md`).
    if runtime.resources.get::<Arc<AssetWatcher>>().is_none() {
        match AssetWatcher::new(assets_root) {
            Ok(watcher) => {
                runtime.resources.insert(Arc::new(watcher));
                log::info!("scripts::mount: watching {}", assets_root.display());
            }
            Err(e) => log::warn!(
                "scripts::mount: hot-reload disabled for {}: {:#}",
                assets_root.display(),
                e
            ),
        }
    } else {
        log::info!(
            "scripts::mount: a watcher is already registered — {} is loaded but not watched",
            assets_root.display()
        );
    }

    let script_dir = assets_root.join("scripts");
    if !script_dir.is_dir() {
        log::info!(
            "scripts::mount: no {} — nothing to compile yet",
            script_dir.display()
        );
        return 0;
    }

    let Some(pending) = runtime
        .resources
        .get::<khora_io::script_hot_reload::PendingReloads>()
    else {
        // The engine bootstrap inserts this. Reaching here means `mount` was
        // called against a runtime the engine never built.
        log::error!("scripts::mount: no reload channel in the runtime — scripts will not load");
        return 0;
    };

    let loaded = khora_io::script_hot_reload::load_all(&script_dir, pending);
    log::info!(
        "scripts::mount: compiled {loaded} script module(s) from {}",
        script_dir.display()
    );
    loaded
}
