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

//! The project indexes built off the frame, taken in at its boundary.
//!
//! The `loading` family stages each project index in the runtime's reserve;
//! `asset_index_intake` takes them in after `asset_intake`, so the decodes
//! read for the asset service serving now are installed in it first. An
//! `Open` builds a new asset service and swaps it into the runtime's; a
//! `Reindex` reindexes the runtime's. Each outcome is filed under its
//! ticket in the runtime's `IndexHub`.

use std::sync::{Arc, Mutex};

use khora_core::asset::{IndexKind, IndexedAssets};
use khora_core::lane::{OutputDeck, Reserve};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, TickPhase, World};
use khora_telemetry::MetricsRegistry;

use crate::asset::{AssetService, FileLoader, IndexHub, IndexOutcome};

/// Takes in every project index the reserve holds, into the runtime's asset
/// service, and files each outcome in the runtime's `IndexHub`. Nothing
/// without a hub or a reserve.
pub fn take_in_indexes(runtime: &Runtime) {
    let (Some(hub), Some(reserve)) = (
        runtime.resources.get::<Arc<IndexHub>>(),
        runtime.look_up::<Reserve>(),
    ) else {
        return;
    };
    for indexed in reserve.take::<IndexedAssets>() {
        let ticket = indexed.ticket;
        // An index whose project was replaced since it was received — an
        // `Open` filed meanwhile — is taken in nowhere: its ticket already
        // says so.
        if !hub.is_pending(ticket) {
            continue;
        }
        let outcome = take_in(runtime, indexed);
        if let Err(why) = &outcome {
            log::warn!("project index {}: {why}", ticket.0);
        }
        hub.land(ticket, outcome);
    }
}

/// Builds or reindexes the runtime's asset service from `indexed`.
fn take_in(runtime: &Runtime, indexed: IndexedAssets) -> IndexOutcome {
    let bytes = indexed.index?;
    let service = runtime
        .services
        .get::<Arc<Mutex<AssetService>>>()
        .ok_or_else(|| "the engine holds no asset service".to_owned())?;
    let mut service = service
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match indexed.kind {
        IndexKind::Reindex => {
            // The engine may serve another project now — a host opened one
            // without filing it here: the index is for the one it was filed
            // for.
            if service.project_root() != Some(indexed.project_root.as_path()) {
                return Err(format!(
                    "the engine no longer serves {}",
                    indexed.project_root.display()
                ));
            }
            service
                .reindex(&bytes)
                .map_err(|error| format!("{error:#}"))
        }
        IndexKind::Open => {
            let assets_root = indexed.project_root.join("assets");
            let io = Box::new(FileLoader::new(&assets_root));
            let mut opened = AssetService::new(&bytes, io, Arc::new(MetricsRegistry::new()), None)
                .map_err(|error| format!("{error:#}"))?;
            opened.register_default_decoders(&assets_root);
            opened.serve_project(&indexed.project_root);
            // The engine holds the service it serves by this one handle: the
            // project opened takes its place behind it.
            std::mem::swap(&mut *service, &mut opened);
            // A uuid is its path unless the registry froze it, so another
            // project's `textures/a.png` is the same uuid: what the engine took
            // from the service moving out may be stale now.
            service.announce_replacing(&opened);
            // The `loading` family takes from the channel the service moving
            // out filed on: the one moving in files there now.
            service.adopt_decode_requests(&mut opened);
            Ok(())
        }
    }
}

fn asset_index_intake_system(
    _world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    take_in_indexes(runtime);
}

inventory::submit! {
    DataSystemRegistration {
        name: "asset_index_intake",
        phase: TickPhase::Boundary,
        run: asset_index_intake_system,
        order_hint: 0,
        // The decodes read for the service serving now are installed in it
        // before a project opened takes its place.
        runs_after: &["asset_intake"],
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}
