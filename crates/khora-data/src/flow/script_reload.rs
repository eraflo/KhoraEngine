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

//! Getting a recompiled module to the agent that runs it.
//!
//! # Why this is not simpler
//!
//! The compiled programs live in the **agent**, which is what lets it declare
//! [`AgentAccess::Isolated`] — an agent that reached into a shared cache every
//! frame could not. But hot-reload happens on the other side of that line: a
//! file changed, a `DataSystem` recompiled it, and the result has to cross to an
//! agent that may not read shared state.
//!
//! The bus is the one channel that crosses it legally. A flow is what publishes
//! into the bus, so the recompiled modules are drained here and handed over as
//! a view — the same road the agent already takes for everything else it reads.
//!
//! # Changes, not state
//!
//! The view carries the modules recompiled **this frame**, which on almost every
//! frame is none. Publishing the whole program set instead would mean cloning
//! every module every frame to deliver something that changes when an author
//! saves a file.

use std::sync::{Arc, Mutex};

use khora_core::Runtime;
use khora_script::vm::Program;

use crate::ecs::{SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;

/// One module that finished recompiling.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptReload {
    /// The module's path, which is its identity in the runtime.
    pub module: String,
    /// What it compiled to.
    pub program: Program,
}

/// Where a recompiled module waits for the next frame.
///
/// Shared, because the two ends are a `DataSystem` and a flow and neither owns
/// the other. This is the *only* shared thing on the path — the agent reads the
/// view, never this, which is what keeps its isolation claim true.
#[derive(Debug, Clone, Default)]
pub struct PendingScriptReloads {
    queue: Arc<Mutex<Vec<ScriptReload>>>,
}

impl PendingScriptReloads {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a recompiled module.
    ///
    /// Replaces an earlier entry for the same module rather than appending: two
    /// saves between frames are one reload, and applying the older one after
    /// the newer would leave the game running code the author already replaced.
    pub fn push(&self, reload: ScriptReload) {
        let Ok(mut queue) = self.queue.lock() else {
            log::error!(
                "script reload queue is poisoned; dropping {}",
                reload.module
            );
            return;
        };
        match queue.iter_mut().find(|held| held.module == reload.module) {
            Some(held) => *held = reload,
            None => queue.push(reload),
        }
    }

    /// Takes everything queued.
    pub fn drain(&self) -> Vec<ScriptReload> {
        match self.queue.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(_) => Vec::new(),
        }
    }

    /// How many are waiting.
    pub fn len(&self) -> usize {
        self.queue.lock().map(|queue| queue.len()).unwrap_or(0)
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The modules recompiled this frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScriptReloadView {
    /// What changed, in the order it was recompiled.
    pub reloaded: Vec<ScriptReload>,
}

impl ScriptReloadView {
    /// Whether anything changed.
    pub fn is_empty(&self) -> bool {
        self.reloaded.is_empty()
    }
}

/// Drains the pending reloads into the bus.
#[derive(Default)]
pub struct ScriptReloadFlow;

impl Flow for ScriptReloadFlow {
    type View = ScriptReloadView;
    const DOMAIN: SemanticDomain = SemanticDomain::Script;
    const NAME: &'static str = "script_reload";

    fn project(&self, _world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        let Some(pending) = runtime.resources.get::<PendingScriptReloads>() else {
            // No watcher installed — a packed build, or a project with no
            // assets directory. Hot-reload is a development affordance and its
            // absence is the shipping path, not a failure.
            return ScriptReloadView::default();
        };

        // Drained rather than read: a reload applies once. Leaving it queued
        // would re-apply it every frame, and re-applying a reload resets an
        // instance's fields to what the file says instead of what the game has
        // since made of them.
        ScriptReloadView {
            reloaded: pending.drain(),
        }
    }
}

register_flow!(ScriptReloadFlow);

#[cfg(test)]
mod tests {
    use super::*;

    fn reload(module: &str) -> ScriptReload {
        ScriptReload {
            module: module.to_owned(),
            program: Program::default(),
        }
    }

    #[test]
    fn a_queued_reload_comes_back_once() {
        let pending = PendingScriptReloads::new();
        pending.push(reload("ai/guard.erg"));

        assert_eq!(pending.len(), 1);
        assert_eq!(pending.drain().len(), 1);
        assert!(pending.is_empty(), "draining takes it");
    }

    /// **Two saves between frames are one reload.** Applying the older after
    /// the newer would leave the game running code the author already replaced.
    #[test]
    fn saving_twice_before_a_frame_queues_one_reload() {
        let pending = PendingScriptReloads::new();
        pending.push(reload("ai/guard.erg"));
        pending.push(reload("ai/guard.erg"));

        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn different_modules_queue_separately() {
        let pending = PendingScriptReloads::new();
        pending.push(reload("ai/guard.erg"));
        pending.push(reload("loot/chest.erg"));

        assert_eq!(pending.drain().len(), 2);
    }

    /// The later save wins, which is the whole reason a repeat replaces rather
    /// than appends.
    #[test]
    fn the_later_save_is_the_one_kept() {
        let pending = PendingScriptReloads::new();
        pending.push(reload("ai/guard.erg"));

        let mut newer = reload("ai/guard.erg");
        newer.program.strings.push("second".to_owned());
        pending.push(newer);

        let drained = pending.drain();
        assert_eq!(drained[0].program.strings, vec!["second"]);
    }

    /// A project with no watcher is the shipping path, not a failure.
    #[test]
    fn a_runtime_without_a_queue_projects_nothing() {
        let view = ScriptReloadFlow.project(&World::new(), &Selection::new(), &Runtime::default());
        assert!(view.is_empty());
    }

    /// **A reload applies once.** Leaving it queued would re-apply it every
    /// frame, resetting an instance to what the file says instead of what the
    /// game has since made of it.
    #[test]
    fn the_flow_drains_rather_than_reads() {
        let pending = PendingScriptReloads::new();
        pending.push(reload("ai/guard.erg"));

        let mut runtime = Runtime::default();
        runtime.resources.insert(pending.clone());

        let first = ScriptReloadFlow.project(&World::new(), &Selection::new(), &runtime);
        assert_eq!(first.reloaded.len(), 1);

        let second = ScriptReloadFlow.project(&World::new(), &Selection::new(), &runtime);
        assert!(second.is_empty(), "the same reload did not arrive twice");
    }
}
