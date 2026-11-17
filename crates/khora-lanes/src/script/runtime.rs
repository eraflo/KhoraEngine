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

//! What scripting keeps between frames, outside the `World`: the compiled
//! programs and each instance's state, locked by the script lane as an input.
//!
//! See the book: concepts/scripting/under-the-hood.md, "Who owns what".

use std::collections::HashMap;
use std::sync::Arc;

use crate::script::ScriptRunReport;
use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_script::arena::{Arena, PersistentStore};
use khora_script::vm::Value;
use khora_script::vm::{BehaviorLayout, Machine, Program};
use serde::{Deserialize, Serialize};

pub use super::fuel_demand::FuelDemand;

/// Instructions per millisecond, before anything has been measured.
///
/// Only the starting point: the first frame that runs corrects it.
pub const INITIAL_RATE: f64 = 50_000.0;

/// What a turn is priced at before one has run whole: a guess, corrected by
/// the first turn that completes.
pub const UNKNOWN_TURN: std::time::Duration = std::time::Duration::from_micros(2);

/// How much of the measured rate one frame's observation may move it.
///
/// Smoothed rather than replaced, because one frame is a noisy sample: a
/// scheduler hiccup would otherwise halve the budget for the frame after it,
/// producing a stutter out of a measurement artefact.
pub(super) const RATE_BLEND: f64 = 0.1;

/// One behavior instance's state.
#[derive(Debug, Default)]
pub struct Instance {
    /// The module its behavior was compiled from.
    ///
    /// Recorded because an entity that leaves the view still has an `OnDespawn`
    /// to run, and by then the view does not say any more which program to run it
    /// from — the instance has to carry the answer with it.
    pub module: String,
    /// Its fields — every slot its layout has, from its first turn on: built
    /// from the layout's data, or from what it arrived with on top of that.
    pub fields: PersistentStore,
    /// Whether `OnSpawn` has run.
    ///
    /// A hot reload keeps it: an edit to a script is not a new entity, and a
    /// guard that has already announced itself should not do it again because
    /// its author changed a number.
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

    /// A body stopped part-way, by an `await` or by fuel, and when to resume it.
    ///
    /// One per instance: a behavior is part-way through one sequence at a time,
    /// whatever body it is. [`Pending::body`] says what finishing it completes.
    pub pending: Option<Pending>,

    /// The fault that disabled the instance, stamped with the code it faulted
    /// in: a save keeps the instance disabled until that code changes.
    pub fault: Option<khora_core::script::RecordedFault>,

    /// Whether `OnLoad` is owed or part-way: the instance was restored from a
    /// save and has not finished announcing it.
    pub loading: bool,

    /// The body a save restored, waiting behind a lifecycle hook part-way —
    /// `OnLoad`, or an `OnSpawn` still owed.
    pub after_load: Option<Pending>,

    /// What the entity brought when it appeared, held until the instance's
    /// first turn.
    pub arriving: Option<khora_data::flow::ScriptArrival>,

    /// The members of bodies part-way through that an edit left nothing of,
    /// each owed its own `OnResumeFailed`, oldest first, one a turn.
    pub resume_failed: Vec<String>,

    /// The program its fields are laid out for: what it last ran, or what a
    /// reload carried it into.
    ///
    /// A reload carries from this layout, so a module saved without the
    /// behavior in between leaves the instance's store as it was.
    pub built_for: Option<Arc<Program>>,

    /// Whether a carry into another program left its state companion in the
    /// World behind it, to be written that frame whether or not it runs.
    pub rewritten: bool,

    /// Game seconds since its last turn, handed to its next turn as its
    /// `delta`: a frame a rate skipped or the fuel missed is not lost time.
    pub owed: f32,

    /// Game seconds its next `Update` is owed past `owed`: time an earlier turn
    /// counted its countdowns down with but stopped before its `Update` ran.
    pub update_owed: f32,

    /// The fixed steps its `FixedUpdate` is owed, each the step it was taken
    /// at, oldest first, up to the engine's step cap; they run first next turn.
    pub fixed_owed: Vec<f32>,

    /// For a rate-limited behavior, the seconds until its next turn is due;
    /// first set at a phase its entity decides, spreading its instances.
    pub until: Option<f32>,

    /// The fuel its last whole turn took, pieces cut across frames summed: what
    /// its next turn is priced at. `None` before its first whole turn.
    pub full_turn: Option<u64>,

    /// The fuel the turn part-way through has spent so far, across the
    /// frames it was cut over.
    pub piece_spent: u64,

    /// The events it was sent with a payload its handler could not take, each
    /// with the last such delivery's reason: one line per event, for a tool.
    pub dropped: std::collections::BTreeMap<String, String>,
}

/// A suspended member and how long is left of its wait.
///
/// Serialisable, so an `async` attack saved half-way through its wind-up loads
/// half-way through it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// The frozen machine.
    pub machine: Machine,
    /// Seconds still to wait — a countdown, for the reason a timer's is. Zero
    /// for a body that ran out of fuel: it resumes next frame.
    pub remaining: f32,
    /// What finishing the machine completes.
    pub body: Body,
    /// The fingerprint of the program the machine stopped in.
    ///
    /// A hot reload that changes the program carries the machine into the reloaded
    /// program through [`resumption::carry`](super::resumption::carry), which
    /// restamps this.
    pub fingerprint: u64,
    /// Game seconds the body has run without finishing since it started or
    /// last resumed from an `await`.
    #[serde(default)]
    pub running: f32,
}

/// Which body a [`Pending`] machine is part-way through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Body {
    /// An event handler, or a member it awaited.
    Sequence,
    /// `OnSpawn`.
    Spawn,
    /// A per-frame member: `Update`, or one step's `FixedUpdate` — counted
    /// when it began, so finishing it completes nothing more.
    Update,
    /// `OnLoad`. Never saved: a load reruns it from the start.
    Load,
    /// A timer's body; finishing it rearms the timer with `rearm`.
    Timer {
        /// The timer's index in the behavior's layout.
        index: usize,
        /// The timer slot's value once the body finishes.
        rearm: Value,
    },
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
    /// Suspended bodies the edit took back below the exact tier, or
    /// abandoned.
    pub resumes: Vec<crate::script::report::Resumed>,
}

impl ReloadReport {
    /// Returns whether anything was lost.
    pub fn lost_anything(&self) -> bool {
        !self.dropped.is_empty()
    }
}

/// Compiled programs and live instances.
///
/// A program is held behind an [`Arc`]: a thousand guards run one `Guard`, and
/// the lane keeps it while the runtime is borrowed mutably for their fields.
#[derive(Debug)]
pub struct ScriptRuntime {
    programs: HashMap<String, Arc<Program>>,
    pub(super) instances: HashMap<(EntityId, String), Instance>,
    /// What behaviors raised for each other, waiting for the next frame.
    ///
    /// The scripting world's own mail, never routed through the `World`.
    /// See the book: concepts/scripting/events.md,
    /// "An event is never delivered in the frame it was raised".
    pub(super) pending: EventQueue,
    /// What the last run did, which the lane prices its next run from.
    last_report: ScriptRunReport,
    /// How many instances the last run found with no compiled module.
    ///
    /// Kept so the warning fires on a transition, not once per frame, where it
    /// would become noise somebody filters out.
    last_unloaded: usize,
    /// Measured instructions per millisecond on this machine.
    ///
    /// Corrected from what each run cost; it turns a time budget into fuel.
    pub(super) rate: f64,
    /// Measured time a lane run costs besides its turns — the frame's
    /// bookkeeping, which no fuel buys. `None` before the first run.
    pub(super) overhead: Option<std::time::Duration>,
    /// The frame memory, lent to each frame's host and given back after.
    ///
    /// One for the scripting world's whole life, so its generation only ever
    /// grows: text a frame built and something kept past it fails to read in a
    /// later frame, rather than reading what that frame built at its index.
    arena: Arena,
    /// Reloads waiting for the frame whose view projects what they read.
    deferred: Vec<khora_script::reload::ScriptReload>,
    /// What it keeps of component-attached instances.
    pub(super) attached: super::attached::Attached,
    /// What each behavior carries from frame to frame of the fuel's sharing.
    pub(super) shares: super::shares::FuelShares,
}

impl Default for ScriptRuntime {
    fn default() -> Self {
        Self {
            programs: HashMap::new(),
            instances: HashMap::new(),
            pending: EventQueue::new(),
            last_report: ScriptRunReport::default(),
            last_unloaded: 0,
            rate: INITIAL_RATE,
            overhead: None,
            arena: Arena::new(),
            deferred: Vec::new(),
            attached: Default::default(),
            shares: Default::default(),
        }
    }
}

impl ScriptRuntime {
    /// Creates an empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Lends out the frame memory, for one frame's host.
    pub(crate) fn take_arena(&mut self) -> Arena {
        std::mem::take(&mut self.arena)
    }

    /// Takes the frame memory back, once the frame has ended and reset it.
    pub(crate) fn return_arena(&mut self, arena: Arena) {
        self.arena = arena;
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
    /// A frame that bails after taking the queue must hand it back, or what was
    /// raised last frame vanishes without a word.
    pub fn set_pending(&mut self, pending: EventQueue) {
        self.pending = pending;
    }

    /// Returns how much mail is waiting, for tests and an inspector.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Records what the run just did.
    pub fn set_last_report(&mut self, report: ScriptRunReport) {
        self.last_report = report;
    }

    /// Returns what the last run did.
    pub fn last_report(&self) -> &ScriptRunReport {
        &self.last_report
    }

    /// Reports instances whose module is not compiled, when the count changes.
    pub fn report_unloaded(&mut self, unloaded: usize) {
        if unloaded == self.last_unloaded {
            return;
        }
        self.last_unloaded = unloaded;

        if unloaded > 0 {
            log::warn!(
                "script lane: {unloaded} instance(s) name a module with no compiled program \
                 — {} module(s) are loaded. Is the project's script root mounted \
                 (`khora_sdk::scripts::mount`)?",
                self.programs.len()
            );
        } else {
            log::info!("script lane: every instance now has a compiled program");
        }
    }

    /// Returns the measured instructions per millisecond.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Registers a compiled module under the path it came from.
    pub fn add_program(&mut self, module: impl Into<String>, program: Program) {
        self.programs.insert(module.into(), Arc::new(program));
    }

    /// Replaces a module, carrying every live instance's state across by name.
    ///
    /// Returns what changed for each behavior, so the caller can tell an author
    /// whose renamed field lost its value. See the book:
    /// concepts/scripting/saves-and-edits.md, "Fields, by name".
    pub fn reload(&mut self, module: &str, program: Program) -> Vec<ReloadReport> {
        let previous = self.programs.get(module).cloned();
        let program = Arc::new(program);
        let mut reports = Vec::new();

        for layout in &program.behaviors {
            let mut report = previous
                .as_ref()
                .and_then(|program| program.layout(&layout.name))
                .map(|old| reload_report(old, layout));

            for ((_, behavior), instance) in self.instances.iter_mut() {
                // Another module may declare a behavior of the same name; its
                // instances run other code and owe this edit nothing.
                if behavior != &layout.name || instance.module != module {
                    continue;
                }
                // An arrival waiting for its turn rebuilds the instance wholly,
                // read against the program it runs then.
                if instance.arriving.is_some() {
                    continue;
                }
                // From the layout its fields were built for: the module may
                // have been saved without the behavior since.
                let Some(old_program) = instance.built_for.clone().or_else(|| previous.clone())
                else {
                    continue;
                };
                let Some(old) = old_program.layout(&layout.name) else {
                    // A behavior its program did not have: nothing to carry.
                    continue;
                };
                let report = report.get_or_insert_with(|| reload_report(old, layout));
                report.resumes.extend(super::carry::carry_instance(
                    instance,
                    &old_program,
                    &program,
                    &layout.name,
                ));
            }

            reports.extend(report);
        }

        self.programs.insert(module.to_owned(), program);
        reports
    }

    /// Takes `module` out of the runtime: its program and any version of it
    /// waiting for a frame.
    ///
    /// Its instances stay, fields and all, for the module coming back.
    pub fn withdraw(&mut self, module: &str) {
        self.programs.remove(module);
        self.deferred.retain(|reload| reload.module != module);
    }

    /// Returns the program compiled from `module`.
    ///
    /// The `Arc`, so a caller keeps it while the runtime is borrowed again for
    /// an instance's fields.
    pub fn program(&self, module: &str) -> Option<&Arc<Program>> {
        self.programs.get(module)
    }

    /// Returns every loaded program.
    pub fn programs(&self) -> impl Iterator<Item = &Arc<Program>> {
        self.programs.values()
    }

    /// Takes the reloads waiting for a frame that projects what they read.
    pub(crate) fn take_deferred(&mut self) -> Vec<khora_script::reload::ScriptReload> {
        std::mem::take(&mut self.deferred)
    }

    /// Keeps `reload` for the next frame.
    pub(crate) fn defer(&mut self, reload: khora_script::reload::ScriptReload) {
        self.deferred.push(reload);
    }

    /// Returns whether a compiled program of `module` waits for the frame that
    /// projects what it reads.
    pub(crate) fn is_waiting(&self, module: &str) -> bool {
        self.deferred.iter().any(|reload| reload.module == module)
    }

    /// Returns the reloads waiting.
    pub(crate) fn deferred(&self) -> &[khora_script::reload::ScriptReload] {
        &self.deferred
    }

    /// Returns how many modules are loaded.
    pub fn module_count(&self) -> usize {
        self.programs.len()
    }

    /// Returns the state of one entity's behavior, created empty if it is new.
    pub fn instance(&mut self, entity: EntityId, behavior: &str) -> &mut Instance {
        self.instances
            .entry((entity, behavior.to_owned()))
            .or_default()
    }

    /// Returns the state of one entity's behavior, if it has any.
    pub fn peek(&self, entity: EntityId, behavior: &str) -> Option<&Instance> {
        self.instances.get(&(entity, behavior.to_owned()))
    }

    /// Returns how many instances are held.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// Returns the instances held, each weighed by the share of `frame`-long
    /// frames it takes a turn in.
    ///
    /// One for an instance with no rate, `rate × frame`, at most one, for a
    /// rate-limited one.
    pub fn weighted_instances(&self, frame: f32) -> f32 {
        self.instances
            .iter()
            .map(|((_, behavior), instance)| {
                let rate = self
                    .program(&instance.module)
                    .and_then(|program| program.layout(behavior))
                    .and_then(|layout| layout.schedule.rate);
                rate.map_or(1.0, |rate| (rate * frame).clamp(0.0, 1.0))
            })
            .sum()
    }
}

/// Returns the member a pending body runs: its outermost function, without
/// the behavior.
pub(super) fn member_of(pending: &Pending, program: &Program) -> String {
    pending
        .machine
        .outermost()
        .and_then(|index| program.functions.get(index))
        .map(|function| {
            function
                .name
                .split_once('.')
                .map_or(function.name.as_str(), |(_, member)| member)
                .to_owned()
        })
        .unwrap_or_default()
}

/// Returns what a reload from `old` to `layout` keeps, adds and drops of a
/// behavior's fields.
fn reload_report(old: &BehaviorLayout, layout: &BehaviorLayout) -> ReloadReport {
    ReloadReport {
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
        resumes: Vec::new(),
    }
}
