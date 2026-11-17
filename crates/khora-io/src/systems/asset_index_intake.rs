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

//! The project indexes built off the frame, taken in at its boundary: an
//! `Open` swaps in a new asset service, a `Reindex` reindexes the current one.

use std::sync::{Arc, Mutex};

use khora_core::asset::{IndexKind, IndexedAssets};
use khora_core::lane::{OutputDeck, Reserve};
use khora_core::sync::LockExt;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, TickPhase, World};
use khora_data::scene::PrefabCatalog;
use khora_telemetry::MetricsRegistry;

use crate::asset::pack_key::stamped_pack_key;
use crate::asset::signing::PackKey;
use crate::asset::{AssetService, FileLoader, IndexHub, IndexOutcome};
use crate::serialization::prefab_catalog;
use crate::trust::{FileGuard, ProjectTrustState, Refusals};

/// Takes every project index the reserve holds into the runtime's asset service.
///
/// Files each outcome in the runtime's `IndexHub`, and makes the runtime's
/// `PrefabCatalog` follow the new index. Nothing without a hub or a reserve.
pub fn take_in_indexes(runtime: &Runtime) {
    let (Some(hub), Some(reserve)) = (
        runtime.resources.get::<Arc<IndexHub>>(),
        runtime.look_up::<Reserve>(),
    ) else {
        return;
    };
    for indexed in reserve.take::<IndexedAssets>() {
        let ticket = indexed.ticket;
        // An `Open` filed meanwhile replaced its project: its ticket already says so.
        if !hub.is_pending(ticket) {
            continue;
        }
        let outcome = take_in(runtime, indexed, stamped_pack_key());
        match &outcome {
            Ok(()) => follow_prefabs(runtime),
            Err(why) => log::warn!("project index {}: {why}", ticket.0),
        }
        hub.land(ticket, outcome);
    }
}

/// Makes the runtime's prefab catalog, when it has one, the prefabs its asset service indexes now.
fn follow_prefabs(runtime: &Runtime) {
    let (Some(catalog), Some(service)) = (
        runtime.resources.get::<PrefabCatalog>(),
        runtime.services.get::<Arc<Mutex<AssetService>>>(),
    ) else {
        return;
    };
    let service = service.lock_recovered();
    catalog.replace(prefab_catalog(&service));
}

/// Builds or reindexes the runtime's asset service from `indexed`.
///
/// A binary `stamped` with a pack key is a built game, which runs from its
/// pack only: it opens no loose project.
fn take_in(runtime: &Runtime, indexed: IndexedAssets, stamped: Option<PackKey>) -> IndexOutcome {
    let bytes = indexed.index?;
    let service = runtime
        .services
        .get::<Arc<Mutex<AssetService>>>()
        .ok_or_else(|| "the engine holds no asset service".to_owned())?;
    let mut service = service.lock_recovered();
    match indexed.kind {
        IndexKind::Reindex => {
            // A host may have opened another project without filing it here.
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
            if stamped.is_some() {
                return Err(format!(
                    "a built game runs from its pack: the loose assets of {} are not opened",
                    indexed.project_root.display()
                ));
            }
            let assets_root = indexed.project_root.join("assets");
            let refusals = Refusals::of(runtime);
            let trust = ProjectTrustState::of(runtime).trust();
            let io = Box::new(FileLoader::guarded(FileGuard::new(
                &assets_root,
                trust,
                Arc::clone(&refusals),
            )));
            let mut opened = AssetService::new(&bytes, io, Arc::new(MetricsRegistry::new()), None)
                .map_err(|error| format!("{error:#}"))?;
            // A trusted open lifts what the project's restricted limits refused.
            if trust == crate::trust::Trust::Project {
                refusals
                    .release_strict_under(crate::trust::Trust::Restricted, &indexed.project_root);
            }
            opened.set_refusals(refusals);
            opened.set_trust(trust);
            opened.register_default_decoders(&assets_root);
            opened.serve_project(&indexed.project_root);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::IndexBuilder;
    use khora_core::asset::IndexTicket;
    use std::path::Path;

    /// A runtime serving an empty asset service, as a game's does before its open lands.
    fn runtime() -> Runtime {
        let mut runtime = Runtime::default();
        runtime
            .services
            .insert(Arc::new(Mutex::new(AssetService::empty())));
        runtime
    }

    /// The `Open` of the project at `project_root`, its `assets/` indexed.
    fn opened(project_root: &Path) -> IndexedAssets {
        let assets = project_root.join("assets");
        std::fs::create_dir_all(assets.join("scenes")).expect("the project's assets");
        std::fs::write(assets.join("scenes/a.kscene"), b"loose scene").expect("a loose file");
        IndexedAssets {
            ticket: IndexTicket(1),
            kind: IndexKind::Open,
            project_root: project_root.to_path_buf(),
            index: IndexBuilder::new(&assets)
                .build_index_bytes()
                .map_err(|e| format!("{e:#}")),
        }
    }

    fn served_root(runtime: &Runtime) -> Option<std::path::PathBuf> {
        let service = runtime
            .services
            .get::<Arc<Mutex<AssetService>>>()
            .expect("a service");
        let root = service
            .lock_recovered()
            .project_root()
            .map(Path::to_path_buf);
        root
    }

    /// **A binary stamped with a pack key opens no loose assets.**
    ///
    /// A built game runs from its pack: the open of a loose project is refused
    /// and the service serves what it served. The same open, in a binary that
    /// carries no key, is taken in.
    #[test]
    fn a_stamped_binary_opens_no_loose_assets() {
        let project = tempfile::tempdir().expect("a temporary folder");
        let stamped = runtime();
        let unstamped = runtime();

        let refused = take_in(&stamped, opened(project.path()), Some(PackKey([7; 32])));
        let taken = take_in(&unstamped, opened(project.path()), None);

        assert!(refused.is_err(), "a stamped binary opened loose assets");
        assert_eq!(served_root(&stamped), None, "the stamped service changed");
        assert_eq!(taken, Ok(()), "an unstamped binary opens its loose assets");
        assert_eq!(served_root(&unstamped).as_deref(), Some(project.path()));
    }
}
