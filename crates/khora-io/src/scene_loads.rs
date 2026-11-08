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

//! Scene loads filed by a host, prepared off the frame, landed at a frame's
//! boundary.
//!
//! A host files a load with [`SceneLoadHub::file`] and keeps its
//! [`LoadTicket`]. The `loading` family prepares it off the frame through
//! the hub, which is the runtime's [`SceneLoading`]; the `scene_intake`
//! DataSystem commits it at a frame's boundary and files its outcome under
//! the ticket, for [`SceneLoadHub::take_outcome`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use khora_core::ecs::EntityReserver;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::Lease;
use khora_core::scene::SceneFile;
use khora_core::Runtime;
use khora_data::ecs::World;
use khora_data::scene::loading::{
    LoadKind, LoadTicket, PreparedLoad, Refile, SceneLoadRequest, SceneLoading, SceneSource,
};
use khora_data::scene::record::{LoadReport, ReportEntry};
use khora_data::scene::{LoadCatalog, LoadFailure, PrefabSource, Prepared};

use crate::serialization::{SerializationService, SerializationServiceError};

/// What a scene load filed off the frame came to: what it adapted, or why
/// it changed nothing.
pub type LoadOutcome = Result<LoadReport, SerializationServiceError>;

/// How many loads may wait to be prepared: past it, a load filed fails at
/// once and says why.
const LOAD_BACKLOG: usize = 64;

/// The world loads are prepared against, as it was at its last boundary.
struct Target {
    catalog: Arc<LoadCatalog>,
    reserver: Arc<EntityReserver>,
    /// The world's catalogue generation, as it moves.
    generation: Arc<AtomicU64>,
}

/// The tickets filed and not landed, the loads prepared and waiting for the
/// loads filed before them, and the outcomes landed and not taken.
#[derive(Default)]
struct Tickets {
    /// In the order filed: a load lands only once every load filed before
    /// it has.
    pending: BTreeSet<LoadTicket>,
    waiting: BTreeMap<LoadTicket, PreparedLoad>,
    /// Withdrawn while in flight: prepared, they land nothing.
    cancelled: HashSet<LoadTicket>,
    landed: HashMap<LoadTicket, LoadOutcome>,
}

/// The scene loads in flight: the requests filed, what they are prepared
/// against, and the outcomes landed, by ticket.
///
/// Its locks are leaves: held only to file, look up or take, never across a
/// call out.
pub struct SceneLoadHub {
    requests: Channel<SceneLoadRequest>,
    next: AtomicU64,
    target: Mutex<Option<Arc<Target>>>,
    tickets: Mutex<Tickets>,
}

impl SceneLoadHub {
    fn new() -> Self {
        Self {
            requests: Channel::bounded(LOAD_BACKLOG, WhenFull::Reject),
            next: AtomicU64::new(1),
            target: Mutex::new(None),
            tickets: Mutex::new(Tickets::default()),
        }
    }

    /// Files `kind` to be loaded into `world`, reading the prefabs it links
    /// to from `prefabs`: prepared off the frame against `world`'s catalogue
    /// as it is now, its ids held from the slots `world` freed, and
    /// committed at a later frame's boundary. Returns what its outcome will
    /// be filed under.
    pub fn file(
        &self,
        world: &mut World,
        kind: LoadKind,
        prefabs: Arc<dyn PrefabSource + Send + Sync>,
    ) -> LoadTicket {
        self.publish(world);
        let ticket = LoadTicket(self.next.fetch_add(1, Ordering::Relaxed));
        self.tickets().pending.insert(ticket);
        let dropped = self.requests.dropped();
        self.requests.send(SceneLoadRequest {
            ticket,
            kind,
            prefabs,
            hold: Some(world.hold_freed()),
        });
        // Refused by a full backlog: the request, and its hold, are gone.
        if self.requests.dropped() != dropped {
            let message = format!("{LOAD_BACKLOG} scene loads are already waiting");
            log::warn!("scene load {}: refused, {message}", ticket.0);
            self.land(
                ticket,
                Err(SerializationServiceError::LoadFailed(failed(message))),
            );
        }
        ticket
    }

    /// Takes the outcome of the load filed as `ticket`, once it landed:
    /// `None` while it is in flight, and once taken.
    pub fn take_outcome(&self, ticket: LoadTicket) -> Option<LoadOutcome> {
        self.tickets().landed.remove(&ticket)
    }

    /// Whether the load filed as `ticket` is still in flight.
    pub fn is_pending(&self, ticket: LoadTicket) -> bool {
        self.tickets().pending.contains(&ticket)
    }

    /// Withdraws the load filed as `ticket`, while it is in flight: it lands
    /// nothing, and files no outcome. What a later action that supersedes it
    /// does — a new scene made while another opens.
    pub fn cancel(&self, ticket: LoadTicket) {
        let withdrawn = {
            let mut tickets = self.tickets();
            if !tickets.pending.remove(&ticket) {
                return;
            }
            let waiting = tickets.waiting.remove(&ticket);
            if waiting.is_none() {
                tickets.cancelled.insert(ticket);
            }
            waiting
        };
        // Dropped past the lock: its hold gives its ids back.
        drop(withdrawn);
    }

    /// Takes the loads `prepared` in, and gives back those whose turn came,
    /// in the order they were filed: each only once every load filed before
    /// it landed or was withdrawn. A withdrawn load is dropped.
    pub(crate) fn in_turn(&self, prepared: Vec<PreparedLoad>) -> Vec<PreparedLoad> {
        let mut withdrawn = Vec::new();
        let mut turn = Vec::new();
        {
            let mut tickets = self.tickets();
            for load in prepared {
                if tickets.cancelled.remove(&load.ticket) {
                    withdrawn.push(load);
                } else {
                    tickets.waiting.insert(load.ticket, load);
                }
            }
            let Tickets {
                pending, waiting, ..
            } = &mut *tickets;
            for ticket in pending.iter() {
                match waiting.remove(ticket) {
                    Some(load) => turn.push(load),
                    None => break,
                }
            }
        }
        drop(withdrawn);
        turn
    }

    /// Puts back the loads of `turn` not landed — one filed again stops the
    /// turn — so they wait for it.
    pub(crate) fn wait_again(&self, loads: impl IntoIterator<Item = PreparedLoad>) {
        let mut tickets = self.tickets();
        for load in loads {
            tickets.waiting.insert(load.ticket, load);
        }
    }

    /// Publishes `world` as the world loads are prepared against, when its
    /// components changed since it last was — or it is another world.
    pub(crate) fn publish(&self, world: &World) {
        let reserver = world.reserver();
        let generation = world.components().generation();
        let mut target = self
            .target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if target.as_ref().is_some_and(|target| {
            target.catalog.generation() == generation && Arc::ptr_eq(&target.reserver, &reserver)
        }) {
            return;
        }
        *target = Some(Arc::new(Target {
            catalog: world.load_catalog(),
            reserver,
            generation: world.catalog_generation(),
        }));
    }

    /// Files `outcome` under `ticket`: the load landed.
    pub(crate) fn land(&self, ticket: LoadTicket, outcome: LoadOutcome) {
        let mut tickets = self.tickets();
        tickets.pending.remove(&ticket);
        tickets.landed.insert(ticket, outcome);
    }

    fn target(&self) -> Option<Arc<Target>> {
        self.target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn tickets(&self) -> MutexGuard<'_, Tickets> {
        // Two sets, whole after every write: read through the poison.
        self.tickets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SceneLoading for SceneLoadHub {
    fn prepare_leased(&self, mut request: SceneLoadRequest) -> (PreparedLoad, Option<Lease>) {
        let again = request.clone();
        let ticket = request.ticket;
        let replaces = !matches!(request.kind, LoadKind::Add(_));
        let refile = Some(Refile {
            requests: self.requests.clone(),
            request: again,
        });
        let Some(target) = self.target() else {
            let prepared = PreparedLoad {
                ticket,
                replaces,
                outcome: Err("no world was published to load into".to_owned()),
                refile,
            };
            return (prepared, None);
        };
        let hold = request
            .hold
            .take()
            .unwrap_or_else(|| target.reserver.hold());
        // A fetch or a decoder that panics fails this load alone: its outcome
        // is filed, so the loads filed after it are not held back for it.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prepare(&request.kind, &target.catalog, hold, &*request.prefabs)
        }))
        .unwrap_or_else(|panic| {
            let why = panic
                .downcast_ref::<&str>()
                .map(|why| (*why).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".to_owned());
            Err(SerializationServiceError::LoadFailed(failed(format!(
                "preparing the load panicked: {why}"
            ))))
        })
        .map_err(|error| error.to_string());
        if let Err(message) = &outcome {
            log::warn!("scene load {}: {message}", ticket.0);
        }
        // Prepared against the catalogue it read: a change of the world's
        // components before the boundary turns it down, and it is prepared
        // again against them.
        let prepared_against = target.catalog.generation();
        let generation = Arc::clone(&target.generation);
        let lease = Lease::new(move || generation.load(Ordering::Acquire) == prepared_against);
        let prepared = PreparedLoad {
            ticket,
            replaces,
            outcome,
            refile,
        };
        (prepared, Some(lease))
    }
}

/// What `kind` loads, read and prepared against `catalog`, its ids held by
/// `hold`.
fn prepare(
    kind: &LoadKind,
    catalog: &LoadCatalog,
    hold: khora_core::ecs::ReservationHold,
    prefabs: &dyn PrefabSource,
) -> Result<(Prepared, Vec<ReportEntry>), SerializationServiceError> {
    match kind {
        LoadKind::Add(source) | LoadKind::Replace(source) => {
            let file = Arc::new(read(source)?);
            SerializationService::prepare_file_with(file, catalog, hold, prefabs)
        }
        LoadKind::Game { save, base } => {
            let (save, base) = (read(save)?, read(base)?);
            SerializationService::prepare_game_with(&save, &base, catalog, hold, prefabs)
        }
    }
}

/// The scene file `source` holds, its bytes fetched when they are not at
/// hand.
fn read(source: &SceneSource) -> Result<SceneFile, SerializationServiceError> {
    let fetched;
    let bytes: &[u8] = match source {
        SceneSource::Bytes(bytes) => bytes,
        SceneSource::Fetch(fetch) => {
            fetched = fetch.fetch().map_err(|error| {
                SerializationServiceError::LoadFailed(failed(format!(
                    "the scene file could not be read: {error}"
                )))
            })?;
            &fetched
        }
    };
    SceneFile::from_bytes(bytes).map_err(|error| {
        SerializationServiceError::LoadFailed(failed(format!(
            "the bytes hold no scene file: {error:?}"
        )))
    })
}

/// A load that failed before it adapted anything.
fn failed(message: String) -> LoadFailure {
    LoadFailure {
        message,
        report: LoadReport::default(),
    }
}

/// Installs in `runtime` what the `loading` family reads — the request
/// channel and the [`SceneLoading`], both through one [`SceneLoadHub`],
/// itself held as `Arc<SceneLoadHub>` for the hosts that file loads.
/// Replaces nothing installed.
pub fn install_scene_loading(runtime: &mut Runtime) {
    if runtime.resources.get::<Arc<SceneLoadHub>>().is_some() {
        return;
    }
    let hub = Arc::new(SceneLoadHub::new());
    runtime.resources.insert(hub.requests.clone());
    let loading: Arc<dyn SceneLoading> = Arc::clone(&hub) as Arc<dyn SceneLoading>;
    runtime.resources.insert(loading);
    runtime.resources.insert(hub);
}
