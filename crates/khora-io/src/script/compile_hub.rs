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

//! Script mounts compiled off the frame, landed at a frame's boundary.
//!
//! A host mounting a project's scripts files their compile with
//! [`CompileHub::file`] and keeps its [`CompileTicket`]. The `loading`
//! family compiles it off the frame and stages a [`CompileDone`] carrying the
//! ticket beside what it compiled; the `script_compile_intake` DataSystem
//! files the outcome under the ticket once the declarations are registered
//! and the programs queued, for [`CompileHub::take_outcome`].

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::event::Channel;
use khora_core::lane::Reserve;
use khora_core::Runtime;
use khora_script::toolchain::ScriptCompiler;

pub use khora_script::toolchain::{CompileDone, CompileRequest, CompileTicket};

/// What a compile filed by ticket came to: how many modules compiled, or why
/// it did not land.
pub type CompileOutcome = Result<usize, String>;

/// The tickets filed and not landed, each with the root it compiles, and the
/// outcomes landed and not taken.
#[derive(Default)]
struct CompileTickets {
    pending: BTreeMap<CompileTicket, PathBuf>,
    landed: HashMap<CompileTicket, CompileOutcome>,
}

/// The script compiles filed by ticket: the requests sent, and the outcomes
/// landed, by ticket.
///
/// Its locks are leaves: held only to file, look up or take, never across a
/// call out.
pub struct CompileHub {
    requests: Channel<CompileRequest>,
    next: AtomicU64,
    tickets: Mutex<CompileTickets>,
}

impl CompileHub {
    /// A hub filing its requests on `requests` — the channel the `loading`
    /// family takes them from.
    pub fn new(requests: Channel<CompileRequest>) -> Self {
        Self {
            requests,
            next: AtomicU64::new(1),
            tickets: Mutex::new(CompileTickets::default()),
        }
    }

    /// Files `modules` of the assets root `root` to be compiled off the
    /// frame, and landed at a later frame's boundary. Returns what its
    /// outcome will be filed under. A request the channel cannot take lands
    /// an error at once; none waiting is dropped for it.
    pub fn file(
        &self,
        root: impl Into<PathBuf>,
        modules: impl IntoIterator<Item = String>,
    ) -> CompileTicket {
        let ticket = CompileTicket(self.next.fetch_add(1, Ordering::Relaxed));
        let mut request = CompileRequest::new(root, modules);
        request.tickets.insert(ticket);
        let root = request.root.clone();
        let mut tickets = self.tickets();
        // The channel refuses a request it cannot hold: nothing waiting is
        // dropped for this one.
        let refused = self.requests.dropped();
        self.requests.send(request);
        if self.requests.dropped() > refused {
            tickets.landed.insert(
                ticket,
                Err("too many compiles already wait; this one was refused".to_owned()),
            );
        } else {
            tickets.pending.insert(ticket, root);
        }
        ticket
    }

    fn tickets(&self) -> std::sync::MutexGuard<'_, CompileTickets> {
        self.tickets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Takes the outcome of the compile filed as `ticket`, once it landed:
    /// `None` while it is in flight, and once taken.
    pub fn take_outcome(&self, ticket: CompileTicket) -> Option<CompileOutcome> {
        self.tickets().landed.remove(&ticket)
    }

    /// Whether the compile filed as `ticket` is still in flight.
    pub fn is_pending(&self, ticket: CompileTicket) -> bool {
        self.tickets().pending.contains_key(&ticket)
    }

    /// Lands `Err("unmounted")` for every ticket of `root` still in flight:
    /// what an unmount does before it drops the root's requests.
    pub fn withdraw(&self, root: &Path) {
        let mut tickets = self.tickets();
        let withdrawn: Vec<CompileTicket> = tickets
            .pending
            .iter()
            .filter(|(_, of)| of.as_path() == root)
            .map(|(ticket, _)| *ticket)
            .collect();
        for ticket in withdrawn {
            tickets.pending.remove(&ticket);
            tickets.landed.insert(ticket, Err("unmounted".to_owned()));
        }
    }

    /// Files `outcome` under `ticket`: the compile landed. A ticket no longer
    /// in flight — withdrawn meanwhile — keeps the outcome it landed with.
    pub(crate) fn land(&self, ticket: CompileTicket, outcome: CompileOutcome) {
        let mut tickets = self.tickets();
        if tickets.pending.remove(&ticket).is_some() {
            tickets.landed.insert(ticket, outcome);
        }
    }
}

/// Compiles at once every request waiting on `runtime`'s channel, through
/// the runtime's `ScriptCompiler`, stages what each hands on from the host,
/// and lands their tickets: what the boot does for a mount filed before it,
/// so a game's `setup` finds what its scripts declare.
pub fn compile_filed_now(runtime: &Runtime) {
    let (Some(requests), Some(compiler), Some(reserve)) = (
        runtime.resources.get::<Channel<CompileRequest>>(),
        runtime.resources.get::<Arc<dyn ScriptCompiler>>(),
        runtime.resources.get::<Reserve>(),
    ) else {
        return;
    };
    let hub = runtime.resources.get::<Arc<CompileHub>>();
    // One compile per root, its requests merged, in the order the roots were
    // first filed.
    let mut by_root: Vec<CompileRequest> = Vec::new();
    for request in requests.drain() {
        match by_root
            .iter_mut()
            .find(|merged| merged.root == request.root)
        {
            Some(merged) => {
                merged.modules.extend(request.modules);
                merged.tickets.extend(request.tickets);
            }
            None => by_root.push(request),
        }
    }
    for request in by_root {
        let compiled = compiler.compile(&request);
        let outcome = Ok(compiled.programs());
        // Before any frame, no run is out: the host's staging lands at once.
        let mut staging = khora_core::lane::Staging::new();
        staging.stage(compiled.declarations);
        staging.stage(compiled.reloads);
        reserve.stage_from_host(staging);
        if let Some(hub) = hub {
            for ticket in &request.tickets {
                hub.land(*ticket, outcome.clone());
            }
        }
    }
}
