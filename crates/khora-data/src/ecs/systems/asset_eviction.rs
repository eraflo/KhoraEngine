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

//! GPU asset eviction — reclaims orphaned GPU meshes/materials each frame.
//!
//! The insert-only projection never removes a `GpuMesh`/`GpuMaterial` once
//! uploaded, so despawns and inline material edits leak GPU memory. This system
//! runs the [`AssetEviction`] pass in [`TickPhase::Maintenance`] — after
//! `ecs_maintenance` compacts orphan rows — to diff the cache against live
//! entity references and free the orphans. The [`AssetEviction`] state and the
//! [`AssetStore`] both live in the [`ServiceRegistry`], so the system fetches
//! and ticks them without any manual wiring.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};
use crate::gpu::{AssetEviction, AssetStore};

fn asset_eviction_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(eviction) = runtime.resources.get::<Arc<Mutex<AssetEviction>>>() else {
        return;
    };
    let Some(store) = runtime.resources.get::<AssetStore>() else {
        return;
    };
    // No graphics device (e.g. headless tick) → nothing to reclaim yet.
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    if let Ok(mut guard) = eviction.lock() {
        guard.tick(store, world, device.as_ref());
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "asset_eviction",
        phase: TickPhase::Maintenance,
        run: asset_eviction_system,
        // After `ecs_maintenance` (order_hint 0) so orphan rows are compacted
        // first; either order is correct since the query skips orphan rows.
        order_hint: 1,
        runs_after: &["ecs_maintenance"],
    }
}
