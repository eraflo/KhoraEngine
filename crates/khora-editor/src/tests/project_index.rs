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

//! The engine's side of a project index, played by hand for the editor's
//! tests: the asset service the project moves into, the index hub, the
//! `loading` family's run that builds what was filed, and the boundary's
//! `asset_intake` and `asset_index_intake` that land it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::asset::{AssetIndexing, IndexRequest};
use khora_sdk::khora_core::event::Channel;
use khora_sdk::khora_core::lane::{OutputDeck, Reserve, Staging};
use khora_sdk::khora_data::ecs::{DataSystemRegistration, SystemContext, Tick, World};
use khora_sdk::{install_indexing, AssetService, IndexHub, Runtime};

use crate::project_vfs::{no_assets, ProjectVfs};

/// A runtime holding what a project index needs: the reserve the
/// background stages into, the engine's asset service and the index hub.
pub(crate) struct IndexingEngine {
    pub runtime: Runtime,
    pub shared: Arc<Mutex<AssetService>>,
    pub hub: Arc<IndexHub>,
    /// How many index requests the background built, in all.
    built: AtomicUsize,
    /// The most requests waiting at one run of the background.
    most_waiting: AtomicUsize,
}

impl IndexingEngine {
    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(no_assets()));
        let mut runtime = Runtime::new();
        runtime.resources.insert(Reserve::new());
        runtime.services.insert(Arc::clone(&shared));
        install_indexing(&mut runtime);
        let hub = runtime
            .resources
            .get::<Arc<IndexHub>>()
            .cloned()
            .expect("`install_indexing` installs the hub");
        Self {
            runtime,
            shared,
            hub,
            built: AtomicUsize::new(0),
            most_waiting: AtomicUsize::new(0),
        }
    }

    /// `vfs` serves the engine's assets and files its reindexes with the
    /// engine's hub, as `before_agents` arranges for the open project.
    pub fn serve(&self, vfs: &Arc<Mutex<ProjectVfs>>) {
        let mut pvfs = vfs.lock().expect("the project");
        pvfs.share_assets(&self.shared);
        pvfs.index_with(Arc::clone(&self.hub));
    }

    fn requests(&self) -> Channel<IndexRequest> {
        self.runtime
            .resources
            .get::<Channel<IndexRequest>>()
            .cloned()
            .expect("the runtime holds the index requests")
    }

    /// How many index requests wait to be built now.
    pub fn waiting(&self) -> usize {
        self.requests().len()
    }

    /// How many index requests were filed so far: built, or waiting.
    pub fn filed(&self) -> usize {
        self.built.load(Ordering::Relaxed) + self.waiting()
    }

    /// The most index requests that waited at once, at a run of the
    /// background.
    pub fn most_waiting(&self) -> usize {
        self.most_waiting.load(Ordering::Relaxed)
    }

    /// One run of the `loading` family: the requests filed are built, each
    /// staged under its lease and received by the reserve, as the next
    /// frame start receives them. The project's files are walked now.
    pub fn build(&self) {
        let indexing = self
            .runtime
            .resources
            .get::<Arc<dyn AssetIndexing>>()
            .cloned()
            .expect("the runtime holds the indexer");
        let reserve = self
            .runtime
            .resources
            .get::<Reserve>()
            .cloned()
            .expect("the runtime holds the reserve");
        let requests = self.requests().drain();
        self.most_waiting
            .fetch_max(requests.len(), Ordering::Relaxed);
        self.built.fetch_add(requests.len(), Ordering::Relaxed);
        let mut staging = Staging::new();
        for request in requests {
            let (indexed, lease) = indexing.index_leased(&request);
            match lease {
                Some(lease) => staging.stage_while(vec![indexed], lease),
                None => staging.stage(vec![indexed]),
            }
        }
        reserve.receive(staging);
    }

    /// The boundary: `asset_intake`, then `asset_index_intake`, which lands
    /// what was built in the engine's service and files its outcome.
    pub fn intake(&self) {
        let mut world = World::new();
        for name in ["asset_intake", "asset_index_intake"] {
            let system = khora_sdk::inventory::iter::<DataSystemRegistration>
                .into_iter()
                .find(|system| system.name == name)
                .unwrap_or_else(|| panic!("`{name}` is not registered"));
            (system.run)(
                &mut world,
                &self.runtime,
                &mut OutputDeck::new(),
                SystemContext {
                    since: Tick(0),
                    resumed: false,
                    scope: None,
                },
            );
        }
    }

    /// The engine's side of one frame: what was filed is built, then landed.
    pub fn frame(&self) {
        self.build();
        self.intake();
    }

    /// Whether the engine's service indexes `uuid`.
    pub fn indexes(&self, uuid: &khora_sdk::khora_core::asset::AssetUUID) -> bool {
        self.shared
            .lock()
            .expect("the service")
            .index()
            .get_metadata(uuid)
            .is_some()
    }
}
