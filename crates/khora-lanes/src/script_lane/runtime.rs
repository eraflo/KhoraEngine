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

//! What scripting keeps between frames.
//!
//! The compiled programs and each instance's fields — everything that outlives a
//! frame but is not part of the scene. It is deliberately **not** in the
//! `World`: a lane may not touch that (`RULES.md` §3), and putting the fields
//! there would mean either the lane writing them or the whole store crossing the
//! boundary twice a frame.
//!
//! Nor is it shared. The agent owns one of these outright, the way it owns its
//! lane registry, so the script lane touches nothing another agent can see —
//! which is what lets the agent declare [`AgentAccess::Isolated`] honestly
//! rather than by assertion.
//!
//! [`AgentAccess::Isolated`]: khora_core::agent::AgentAccess::Isolated

use std::collections::HashMap;
use std::sync::Arc;

use crate::script_lane::ScriptRunReport;
use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::vm::Value;
use khora_script::vm::{BehaviorLayout, Machine, Program};
use serde::{Deserialize, Serialize};

/// One behavior instance's state.
#[derive(Debug, Default)]
pub struct Instance {
    /// The module its behavior was compiled from.
    ///
    /// Recorded because an entity that leaves the view still has an `OnDespawn`
    /// to run, and by then the view no longer says which program to run it
    /// from — the instance has to carry the answer with it.
    pub module: String,
    /// Its fields.
    pub fields: PersistentStore,
    /// Whether its declared defaults have been applied.
    ///
    /// A default is an expression, so it takes a run to produce; this is what
    /// stops that run happening again every frame.
    pub initialised: bool,
    /// Whether `OnSpawn` has run.
    ///
    /// Deliberately not [`initialised`](Self::initialised), which a hot-reload
    /// clears so the new program's defaults are produced. An edit to a script is
    /// not a new entity: a guard that has already announced itself should not do
    /// it again because its author changed a number.
    pub spawned: bool,
    /// Whether `OnDespawn` has run.
    ///
    /// A script that despawns itself gets its farewell in the same frame, while
    /// the entity is still there to read; the sweep that notices it left the view
    /// happens a frame later and must not repeat it.
    pub farewelled: bool,
    /// Whether the instance has faulted and stopped being called.
    ///
    /// A faulty script must not take the frame down, and must not be retried
    /// forever either — a behavior that faults every frame would fill the log
    /// and spend the budget doing it.
    pub disabled: bool,

    /// A member stopped part-way through an `await`, and when to resume it.
    ///
    /// The machine is kept whole rather than the continuation being rebuilt:
    /// what a suspension captured is a program counter, a register file and a
    /// call stack, and re-deriving those would mean knowing where the code had
    /// got to — which is exactly what the machine already records.
    ///
    /// One per instance, not one per call. A behavior can be part-way through
    /// one sequence at a time; a second `await` starting while the first is
    /// pending would mean two answers to "where is this behavior", and the
    /// language has no syntax for asking which.
    pub pending: Option<Pending>,

    /// Values carried across a reload, to be restored **after** the new
    /// program's initialiser has run.
    ///
    /// Not simply left in [`fields`](Self::fields), because the initialiser
    /// writes every slot: a behavior that gained a field has to run it for the
    /// new slot to get its declared default, and that run would overwrite the
    /// values the reload just carried. So they are set aside and put back on
    /// top — the new field gets its default, the old ones keep what they had.
    pub carried: Option<PersistentStore>,
}

/// A suspended member and how long is left of its wait.
///
/// Serialisable, which is what makes saving a scene mid-sequence possible: an
/// `async` attack half-way through its wind-up loads half-way through it. That
/// was proven of the machine before any syntax existed; this is where the
/// promise is finally kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// The frozen machine.
    pub machine: Machine,
    /// Seconds still to wait — a countdown, for the reason a timer's is.
    pub remaining: f32,
}

/// What a reload did to one behavior's instances.
///
/// Reported rather than logged here, so the caller decides where it goes — the
/// editor's Console while playing, the terminal for a headless run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadReport {
    /// Which behavior.
    pub behavior: String,
    /// Fields that survived the edit with their values.
    pub kept: Vec<String>,
    /// Fields the edit introduced, which take their declared defaults.
    pub added: Vec<String>,
    /// Fields the edit removed, whose values are gone.
    ///
    /// The one an author most wants to hear about: a renamed field looks like
    /// one dropped and one added, and its value did not travel.
    pub dropped: Vec<String>,
}

impl ReloadReport {
    /// Whether anything was lost.
    pub fn lost_anything(&self) -> bool {
        !self.dropped.is_empty()
    }
}

/// Moves an instance's values from the old slot layout to the new one.
///
/// By name, never by position: inserting one field at the top shifts every slot
/// after it, and carrying values positionally would move a guard's health into
/// its speed without a word.
fn remap(current: &PersistentStore, old: &BehaviorLayout, new: &BehaviorLayout) -> PersistentStore {
    let mut next = PersistentStore::with_slots(new.fields.len());
    for (slot, field) in new.fields.iter().enumerate() {
        let Some(was) = old.slot_of(field) else {
            // New field: left unset, because the initialiser will fill it and
            // a zero written here would shadow the author's declared default.
            continue;
        };
        if let Some(value) = current.get(was) {
            next.set(slot, value.clone());
        }
    }
    next
}

/// Writes `carried` over `fields`, skipping the slots it left unset.
///
/// The second half of a reload: the initialiser has just produced the new
/// program's defaults for every slot, and this puts back the values that
/// survived the edit. Unset slots are skipped rather than copied, which is
/// exactly how a newly-added field keeps the default it was just given.
pub fn restore_carried(fields: &mut PersistentStore, carried: &PersistentStore) {
    for slot in 0..carried.len() {
        match carried.get(slot) {
            Some(Persisted::Scalar(Value::Unit)) | None => {}
            Some(value) => fields.set(slot, value.clone()),
        }
    }
}

/// Compiled programs and live instances.
///
/// A program is held behind an [`Arc`] because a thousand guards run *one*
/// `Guard`: the lane needs the program while the runtime is borrowed mutably for
/// the instance's fields, and cloning a `Vec<Instruction>` per instance per
/// frame to get it would make the frame cost scale with the code's size for no
/// reason. The reference count is the whole cost instead.
#[derive(Debug)]
pub struct ScriptRuntime {
    programs: HashMap<String, Arc<Program>>,
    instances: HashMap<(EntityId, String), Instance>,
    /// What behaviors raised for each other, waiting for the next frame.
    ///
    /// Held here rather than routed through the `World` and back: an event from
    /// one behavior to another never leaves scripting, and the round trip would
    /// cost two frames of latency and a deck slot nothing else reads.
    /// Engine-raised events arrive the other way, through a
    /// `Channel<ScriptEvent>` the engine fills and the agent drains.
    ///
    /// It lives on the runtime and not on the agent because an agent that
    /// buffers its own output is an agent that has stopped being a strategist
    /// (`RULES.md` §8). This *is* the scripting world's pending mail, and the
    /// runtime is the scripting world.
    pending: EventQueue,
    /// What the last run did, for the agent's `report_status`.
    ///
    /// Same reason: per-frame numbers do not live as agent state — the
    /// convention `AgentFrameStatusMap` sets for scheduler-measured timings,
    /// applied to the counters only the lane can produce.
    last_report: ScriptRunReport,
    /// How many instances the last run found with no compiled module.
    ///
    /// Kept only so the warning fires on a *transition*: an entity naming a
    /// module nobody compiled would otherwise print once per frame, which is
    /// how a real message becomes noise somebody filters out.
    last_unloaded: usize,
    /// Measured instructions per millisecond on this machine.
    ///
    /// The lane corrects it from what a run actually cost, and the agent reads
    /// it to turn a time budget into fuel. It lives here because only the lane
    /// knows how many instructions were spent, and an agent that measured its
    /// own execution would be doing the lane's job.
    rate: f64,
}

/// Instructions per millisecond, before anything has been measured.
///
/// Only ever the starting point: the first frame that runs corrects it. A
/// constant that stayed fixed would be wrong on every machine but the one it
/// was written on, and wrong in the direction that matters — too generous on a
/// slow machine is exactly where the budget needed to hold.
pub const INITIAL_RATE: f64 = 50_000.0;

/// How much of the measured rate one frame's observation may move it.
///
/// Smoothed rather than replaced, because one frame is a noisy sample: a
/// scheduler hiccup would otherwise halve the budget for the frame after it,
/// producing a stutter out of a measurement artefact.
const RATE_BLEND: f64 = 0.1;

impl Default for ScriptRuntime {
    fn default() -> Self {
        Self {
            programs: HashMap::new(),
            instances: HashMap::new(),
            pending: EventQueue::new(),
            last_report: ScriptRunReport::default(),
            last_unloaded: 0,
            rate: INITIAL_RATE,
        }
    }
}

impl ScriptRuntime {
    /// An empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes the mail waiting for delivery, leaving the queue empty.
    ///
    /// Taken rather than borrowed because the lane needs the runtime mutably
    /// while it delivers, and an event still in the queue while it is being
    /// delivered is one that can be delivered twice.
    pub fn take_pending(&mut self) -> EventQueue {
        std::mem::take(&mut self.pending)
    }

    /// Puts mail back, to be delivered by a frame that can.
    ///
    /// The failure path matters more than the happy one: a frame that bails
    /// after taking the queue must return it, or everything raised last frame
    /// vanishes — silently, because an event nobody was told about looks
    /// exactly like one nobody raised.
    pub fn set_pending(&mut self, pending: EventQueue) {
        self.pending = pending;
    }

    /// How much mail is waiting. For tests and for an inspector.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Records what the run just did.
    pub fn set_last_report(&mut self, report: ScriptRunReport) {
        self.last_report = report;
    }

    /// What the last run did.
    pub fn last_report(&self) -> &ScriptRunReport {
        &self.last_report
    }

    /// Reports instances whose module is not compiled, when the count changes.
    ///
    /// The failure it names is the one that hid for a long time: the channel
    /// carrying compiled programs was only ever filled by one binary, so in the
    /// editor and the sandbox this table stayed empty and every instance was
    /// passed over without a word.
    pub fn report_unloaded(&mut self, unloaded: usize) {
        if unloaded == self.last_unloaded {
            return;
        }
        self.last_unloaded = unloaded;

        if unloaded > 0 {
            log::warn!(
                "script lane: {unloaded} instance(s) name a module with no compiled program                  — {} module(s) are loaded. Is the project's script root mounted                  (`khora_sdk::scripts::mount`)?",
                self.programs.len()
            );
        } else {
            log::info!("script lane: every instance now has a compiled program");
        }
    }

    /// Measured instructions per millisecond, for turning a budget into fuel.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Corrects the measured rate from what a run actually cost.
    pub fn observe_rate(&mut self, spent: u64, elapsed: std::time::Duration) {
        let millis = elapsed.as_secs_f64() * 1_000.0;
        // A run too short to time says nothing: the clock's own resolution
        // would dominate, and a rate read from noise is worse than the last one
        // that was measured properly.
        if spent == 0 || millis <= f64::EPSILON {
            return;
        }

        let observed = spent as f64 / millis;
        self.rate = self.rate * (1.0 - RATE_BLEND) + observed * RATE_BLEND;
    }

    /// Registers a compiled module under the path it came from.
    pub fn add_program(&mut self, module: impl Into<String>, program: Program) {
        self.programs.insert(module.into(), Arc::new(program));
    }

    /// Replaces a module, carrying every live instance's state across.
    ///
    /// The point of hot-reload, and the part that is not plumbing: an author
    /// edits a guard's `Update` while ten guards are patrolling, and the ten
    /// should still be where they were, with the health they had. Restarting
    /// them would make the feature useless for exactly the thing it is for.
    ///
    /// Fields are matched **by name**, because a slot means nothing across a
    /// recompile — inserting one field at the top shifts every slot after it,
    /// and a positional carry-over would silently move a guard's health into
    /// its speed. A field that survived the edit keeps its value; one that is
    /// new is left for the behavior's initialiser to fill; one that is gone is
    /// dropped.
    ///
    /// Returns what changed, so the caller can say so rather than reloading in
    /// silence — an author who renamed a field and lost its value should be
    /// told, not left to discover it.
    pub fn reload(&mut self, module: &str, program: Program) -> Vec<ReloadReport> {
        let previous = self.programs.get(module).cloned();
        let mut reports = Vec::new();

        for layout in &program.behaviors {
            let old = previous
                .as_ref()
                .and_then(|program| program.layout(&layout.name));
            let Some(old) = old else {
                // A behavior the module did not have before: nothing to carry.
                continue;
            };

            let report = ReloadReport {
                behavior: layout.name.clone(),
                kept: layout
                    .fields
                    .iter()
                    .filter(|field| old.slot_of(field).is_some())
                    .cloned()
                    .collect(),
                added: layout
                    .fields
                    .iter()
                    .filter(|field| old.slot_of(field).is_none())
                    .cloned()
                    .collect(),
                dropped: old
                    .fields
                    .iter()
                    .filter(|field| layout.slot_of(field).is_none())
                    .cloned()
                    .collect(),
            };

            for ((_, behavior), instance) in self.instances.iter_mut() {
                if behavior != &layout.name {
                    continue;
                }
                let carried = remap(&instance.fields, old, layout);
                instance.fields = carried.clone();
                // Re-initialised so the new program's defaults are produced —
                // its literals may have changed too, not only its field list.
                // The carried values go back on top afterwards.
                instance.initialised = false;
                instance.carried = Some(carried);
                // An edit is the author's answer to whatever faulted. Refusing
                // to try again would make a script unfixable without a restart.
                instance.disabled = false;
            }

            reports.push(report);
        }

        self.programs.insert(module.to_owned(), Arc::new(program));
        reports
    }

    /// The program compiled from `module`.
    ///
    /// Handed out as the `Arc` rather than a borrow so a caller can keep it
    /// while the runtime is borrowed again for an instance's fields — which is
    /// the ordinary shape of running a behavior, not an unusual one.
    pub fn program(&self, module: &str) -> Option<&Arc<Program>> {
        self.programs.get(module)
    }

    /// How many modules are loaded.
    ///
    /// Named for what it counts, beside `instance_count`: a bare `len` on a
    /// runtime holding both modules and instances says which of the two only by
    /// convention, and the two numbers are rarely the same.
    pub fn module_count(&self) -> usize {
        self.programs.len()
    }

    /// The state of one entity's behavior, created empty if it is new.
    pub fn instance(&mut self, entity: EntityId, behavior: &str) -> &mut Instance {
        self.instances
            .entry((entity, behavior.to_owned()))
            .or_default()
    }

    /// Instances whose entity the predicate no longer keeps, and which have not
    /// yet said goodbye.
    ///
    /// Returned as `(entity, behavior, module)` rather than acted on here: the
    /// farewell is a *run*, which needs a host and a budget, and neither belongs
    /// to a store of state. The caller runs them and then calls
    /// [`retain_live`](Self::retain_live) to drop them.
    pub fn departed(&self, alive: impl Fn(EntityId) -> bool) -> Vec<(EntityId, String, String)> {
        self.instances
            .iter()
            .filter(|((entity, _), instance)| !instance.farewelled && !alive(*entity))
            .map(|((entity, behavior), instance)| {
                (*entity, behavior.clone(), instance.module.clone())
            })
            .collect()
    }

    /// The state of one entity's behavior, if it has any.
    pub fn peek(&self, entity: EntityId, behavior: &str) -> Option<&Instance> {
        self.instances.get(&(entity, behavior.to_owned()))
    }

    /// Forgets every instance the given predicate does not keep.
    ///
    /// Called with the frame's live entities, which is how a despawned one's
    /// fields are released. Sweeping rather than reacting to a despawn: the
    /// lane never sees the despawn, only the view that no longer contains it,
    /// and deriving the answer means nothing has to be notified.
    pub fn retain_live(&mut self, alive: impl Fn(EntityId) -> bool) {
        self.instances.retain(|(entity, _), _| alive(*entity));
    }

    /// How many instances are held.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}
