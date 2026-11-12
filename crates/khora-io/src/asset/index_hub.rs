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

//! Projects indexed off the frame, landed at a frame's boundary.
//!
//! A host files an index with [`IndexHub::file`] and keeps its
//! [`IndexTicket`]. The `loading` family indexes it off the frame through
//! the hub, which is the runtime's [`AssetIndexing`]; the
//! `asset_index_intake` DataSystem builds or reindexes the engine's asset
//! service at a frame's boundary and files the outcome under the ticket, for
//! [`IndexHub::take_outcome`].

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::asset::{AssetIndexing, IndexKind, IndexRequest, IndexTicket, IndexedAssets};
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::Lease;
use khora_core::Runtime;

/// What an index filed off the frame came to: landed, or why not.
pub type IndexOutcome = Result<(), String>;

/// How many indexes may wait to be built: past it, an index filed fails at
/// once and says why.
const INDEX_BACKLOG: usize = 64;

/// The tickets filed and not landed, each with the project generation it was
/// filed under, and the outcomes landed and not taken.
#[derive(Default)]
struct IndexTickets {
    pending: BTreeMap<IndexTicket, u64>,
    landed: HashMap<IndexTicket, IndexOutcome>,
}

/// The project indexes in flight: the requests filed, and the outcomes
/// landed, by ticket.
///
/// Its locks are leaves: held only to file, look up or take, never across a
/// call out.
pub struct IndexHub {
    requests: Channel<IndexRequest>,
    next: AtomicU64,
    /// Moves when an `Open` is filed: an index filed before it is for a
    /// project replaced since.
    opened: Arc<AtomicU64>,
    tickets: Mutex<IndexTickets>,
    /// What a project's files are read through while indexed: the run's
    /// refusals and the trust of the project open. `None` until both are
    /// installed; the index then reads as it is (a test's).
    through: Mutex<Option<ReadThrough>>,
}

/// The run's refusals and the trust of the project open.
type ReadThrough = (
    Arc<crate::trust::Refusals>,
    Arc<crate::trust::ProjectTrustState>,
);

impl IndexHub {
    fn new() -> Self {
        Self {
            requests: Channel::bounded(INDEX_BACKLOG, WhenFull::Reject),
            next: AtomicU64::new(1),
            opened: Arc::new(AtomicU64::new(0)),
            tickets: Mutex::new(IndexTickets::default()),
            through: Mutex::new(None),
        }
    }

    /// Files the project at `project_root` to be indexed for `kind`, off
    /// the frame, and landed at a later frame's boundary. Returns what its
    /// outcome will be filed under.
    ///
    /// An `Open` replaces the project: every index filed before it and still
    /// in flight lands an error at once, and its result is turned down when
    /// it reaches the boundary.
    pub fn file(&self, project_root: impl Into<PathBuf>, kind: IndexKind) -> IndexTicket {
        let ticket = IndexTicket(self.next.fetch_add(1, Ordering::Relaxed));
        let mut tickets = self.tickets();
        let generation = match kind {
            IndexKind::Open => {
                let replaced = std::mem::take(&mut tickets.pending);
                for earlier in replaced.into_keys() {
                    tickets
                        .landed
                        .insert(earlier, Err("superseded by another project".to_owned()));
                }
                self.opened.fetch_add(1, Ordering::AcqRel) + 1
            }
            IndexKind::Reindex => self.opened.load(Ordering::Acquire),
        };
        let dropped = self.requests.dropped();
        self.requests.send(IndexRequest {
            ticket,
            project_root: project_root.into(),
            kind,
        });
        if self.requests.dropped() > dropped {
            tickets.landed.insert(
                ticket,
                Err(format!(
                    "{INDEX_BACKLOG} indexes already wait to be built; this one was refused"
                )),
            );
        } else {
            tickets.pending.insert(ticket, generation);
        }
        ticket
    }

    fn tickets(&self) -> std::sync::MutexGuard<'_, IndexTickets> {
        self.tickets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes the outcome of the index filed as `ticket`, once it landed:
    /// `None` while it is in flight, and once taken.
    pub fn take_outcome(&self, ticket: IndexTicket) -> Option<IndexOutcome> {
        self.tickets().landed.remove(&ticket)
    }

    /// Whether the index filed as `ticket` is still in flight.
    pub fn is_pending(&self, ticket: IndexTicket) -> bool {
        self.tickets().pending.contains_key(&ticket)
    }

    /// Builds every project index from now on through a guard at the trust
    /// of the project open, `trust` read at each index, reporting to the
    /// run's `refusals`.
    pub fn read_through(
        &self,
        refusals: Arc<crate::trust::Refusals>,
        trust: Arc<crate::trust::ProjectTrustState>,
    ) {
        let mut through = self
            .through
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *through = Some((refusals, trust));
    }

    /// Files `outcome` under `ticket`: the index landed.
    ///
    /// A ticket no longer in flight — replaced by an `Open` meanwhile —
    /// keeps the outcome it landed with.
    pub(crate) fn land(&self, ticket: IndexTicket, outcome: IndexOutcome) {
        let mut tickets = self.tickets();
        if tickets.pending.remove(&ticket).is_some() {
            tickets.landed.insert(ticket, outcome);
        }
    }
}

impl AssetIndexing for IndexHub {
    fn index_leased(&self, request: &IndexRequest) -> (IndexedAssets, Option<Lease>) {
        // Built for the project generation it was filed under: an `Open`
        // filed since turns it down at the boundary.
        let filed_under = self
            .tickets()
            .pending
            .get(&request.ticket)
            .copied()
            .unwrap_or(u64::MAX);
        let opened = Arc::clone(&self.opened);
        let lease = Lease::new(move || opened.load(Ordering::Acquire) == filed_under);
        // A walk or a lister that panics fails this index alone.
        let through = self
            .through
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let index = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            build_index(&request.project_root, through)
        }))
        .unwrap_or_else(|panic| {
            let why = panic
                .downcast_ref::<&str>()
                .map(|why| (*why).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "no message".to_owned());
            Err(format!("indexing the project panicked: {why}"))
        });
        let indexed = IndexedAssets {
            ticket: request.ticket,
            kind: request.kind,
            project_root: request.project_root.clone(),
            index,
        };
        (indexed, Some(lease))
    }
}

/// The index of `<project_root>/assets/`, its ids read from the project's
/// registry, the files it looks into read `through` a guard at the trust of
/// the project open.
fn build_index(
    project_root: &std::path::Path,
    through: Option<ReadThrough>,
) -> Result<Vec<u8>, String> {
    let registry = super::AssetIdRegistry::load(project_root);
    let assets = project_root.join("assets");
    let guard = through.map(|(refusals, trust)| {
        crate::trust::FileGuard::new(&assets, trust.get().trust(), refusals)
    });
    let mut builder = super::IndexBuilder::new(&assets).with_registry(&registry);
    if let Some(guard) = &guard {
        builder = builder.with_guard(guard);
    }
    builder
        .build_index_bytes()
        .map_err(|error| format!("{error:#}"))
}

/// Installs in `runtime` what the `loading` family reads — the request
/// channel and the [`AssetIndexing`], both through one [`IndexHub`],
/// itself held as `Arc<IndexHub>` for the hosts that file indexes.
/// Replaces nothing installed.
pub fn install_indexing(runtime: &mut Runtime) {
    if runtime.resources.get::<Arc<IndexHub>>().is_some() {
        return;
    }
    let hub = Arc::new(IndexHub::new());
    // Installed after the run's refusals, it reads through them at once;
    // installed before, the refusals' install hands them over.
    let refusals = runtime.resources.get::<Arc<crate::trust::Refusals>>();
    let trust = runtime
        .resources
        .get::<Arc<crate::trust::ProjectTrustState>>();
    if let (Some(refusals), Some(trust)) = (refusals, trust) {
        hub.read_through(Arc::clone(refusals), Arc::clone(trust));
    }
    runtime.resources.insert(hub.requests.clone());
    let indexing: Arc<dyn AssetIndexing> = Arc::clone(&hub) as Arc<dyn AssetIndexing>;
    runtime.resources.insert(indexing);
    runtime.resources.insert(hub);
}
