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
use khora_script::arena::{Arena, PersistentStore};
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
    ///
    /// Any body can be that sequence — an event handler, `OnSpawn`, `Update`,
    /// a timer — cut short by an `await` or by fuel; [`Pending::body`] says
    /// what finishing it completes.
    pub pending: Option<Pending>,

    /// The fault that disabled the instance, stamped with the program it
    /// faulted in — recorded, so a save keeps the instance disabled until the
    /// code changes.
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

    /// The program its fields are laid out for — what it last ran, or what a
    /// reload carried it into. A reload carries from this layout, not from
    /// whatever the module held just before: a module saved without the
    /// behavior in between leaves the instance's store as it was.
    pub built_for: Option<Arc<Program>>,

    /// Whether a carry into another program has left the World's record of it
    /// — its state companion — behind what it now is: written the frame of
    /// the carry, whether or not it runs.
    pub rewritten: bool,

    /// Game seconds that passed since its last turn — what its next turn is
    /// handed as its `delta`: a frame a rate skipped, or the fuel could not
    /// reach, is not lost time.
    pub owed: f32,

    /// The fixed steps its `FixedUpdate` has not been called for yet, each
    /// the step it was taken at, oldest first, at most the engine's step cap:
    /// they run first, on its next turn.
    pub fixed_owed: Vec<f32>,

    /// For a rate-limited behavior, the seconds until its next turn is due —
    /// started, the first time it is seen, at a phase its entity decides, so
    /// that its instances spread over the period.
    pub until: Option<f32>,

    /// The events it was sent with a payload its handler could not take —
    /// dropped, not run — by event, the last such delivery's reason: what a
    /// tool shows, one line per event however often it happened.
    pub dropped: std::collections::BTreeMap<String, String>,
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
    /// Seconds still to wait — a countdown, for the reason a timer's is. Zero
    /// for a body that ran out of fuel: it resumes next frame.
    pub remaining: f32,
    /// What finishing the machine completes.
    pub body: Body,
    /// The program the machine stopped in. A machine is a position in that
    /// code; a hot reload that changes it carries the machine into the new code
    /// through [`resumption::carry`](super::resumption::carry), which
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
    pub resumes: Vec<crate::script_lane::report::Resumed>,
}

impl ReloadReport {
    /// Whether anything was lost.
    pub fn lost_anything(&self) -> bool {
        !self.dropped.is_empty()
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
    pub(super) instances: HashMap<(EntityId, String), Instance>,
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
    /// Whether a frame has run here — what tells a play session starting
    /// over from the first one.
    played: bool,
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
            played: false,
            last_report: ScriptRunReport::default(),
            last_unloaded: 0,
            rate: INITIAL_RATE,
            arena: Arena::new(),
            deferred: Vec::new(),
            attached: Default::default(),
            shares: Default::default(),
        }
    }
}

impl ScriptRuntime {
    /// An empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Lends out the frame memory, for one frame's host.
    pub fn take_arena(&mut self) -> Arena {
        std::mem::take(&mut self.arena)
    }

    /// Takes the frame memory back, once the frame has ended and reset it.
    pub fn return_arena(&mut self, arena: Arena) {
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

    /// Records a frame run; whether one had run before it.
    pub fn note_frame(&mut self) -> bool {
        std::mem::replace(&mut self.played, true)
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
                "script lane: {unloaded} instance(s) name a module with no compiled program \
                 — {} module(s) are loaded. Is the project's script root mounted \
                 (`khora_sdk::scripts::mount`)?",
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
    /// new starts at its declared default; one that is gone is
    /// dropped.
    ///
    /// Returns what changed, so the caller can say so rather than reloading in
    /// silence — an author who renamed a field and lost its value should be
    /// told, not left to discover it.
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

    /// Takes `module` out of the runtime: its program, and any version of it
    /// waiting for a frame. Its instances stay, fields and all, for the module
    /// coming back.
    pub fn withdraw(&mut self, module: &str) {
        self.programs.remove(module);
        self.deferred.retain(|reload| reload.module != module);
    }

    /// The program compiled from `module`.
    ///
    /// Handed out as the `Arc` rather than a borrow so a caller can keep it
    /// while the runtime is borrowed again for an instance's fields — which is
    /// the ordinary shape of running a behavior, not an unusual one.
    pub fn program(&self, module: &str) -> Option<&Arc<Program>> {
        self.programs.get(module)
    }

    /// Every loaded program.
    pub fn programs(&self) -> impl Iterator<Item = &Arc<Program>> {
        self.programs.values()
    }

    /// The reloads waiting for a frame that projects what they read, taken.
    pub(crate) fn take_deferred(&mut self) -> Vec<khora_script::reload::ScriptReload> {
        std::mem::take(&mut self.deferred)
    }

    /// Keeps `reload` for the next frame.
    pub(crate) fn defer(&mut self, reload: khora_script::reload::ScriptReload) {
        self.deferred.push(reload);
    }

    /// Whether a compiled program of `module` waits for the frame that
    /// projects what it reads.
    pub(crate) fn is_waiting(&self, module: &str) -> bool {
        self.deferred.iter().any(|reload| reload.module == module)
    }

    /// The reloads waiting, as they are.
    pub(crate) fn deferred(&self) -> &[khora_script::reload::ScriptReload] {
        &self.deferred
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

    /// The state of one entity's behavior, if it has any.
    pub fn peek(&self, entity: EntityId, behavior: &str) -> Option<&Instance> {
        self.instances.get(&(entity, behavior.to_owned()))
    }

    /// How many instances are held.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// The instances held, each weighed by the share of `frame`-long frames
    /// it takes a turn in: one for an instance with no rate, `rate × frame`
    /// for a rate-limited one, never more than one.
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

/// The member a pending body runs: its outermost function, without the
/// behavior.
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

/// What a reload from `old` to `layout` keeps, adds and drops of a behavior's
/// fields.
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
