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

//! **The scripting world takes in what the engine handed it before the
//! script flow projects.** The `script_intake` data system, every frame of
//! play: it drains the reload and event channels, installs each recompiled
//! module whose reads the demand on the script flow covers — the others wait
//! — announces what the installed programs read, and clears the scripting
//! world's own mail when the scripting family resumes. The lane only runs
//! behaviors: it leaves the channels alone.

use std::sync::{Arc, Mutex};

use khora_core::ecs::entity::EntityId;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::projection::Interest;
use khora_core::script::{ComponentName, ScriptEvent, ScriptReads, ScriptValue};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, Role, SystemContext, Tick, TickPhase, World};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{take_in, BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "ai/guard.erg";

/// Counts its frames and the hits it heard; reads no component.
const COUNTING: &str = "behavior Guard {
    int ticks = 0;
    int hits = 0;
    on Hit(int by) { hits += by; }
    void Update(float dt) { ticks += 1; }
}
event Hit(int by);";

/// The guard reading `Marker`.
const READS_MARKER: &str = "component Marker { int x = 0; }
behavior Guard {
    int ticks = 0;
    int hits = 0;
    on Hit(int by) { hits += by; }
    void Update(float dt) { ticks += 1; var m = this.Get(Marker); }
}
event Hit(int by);";

/// The guard reading `Beacon`.
const READS_BEACON: &str = "component Beacon { int y = 0; }
behavior Guard {
    int ticks = 0;
    int hits = 0;
    on Hit(int by) { hits += by; }
    void Update(float dt) { ticks += 1; var b = this.Get(Beacon); }
}
event Hit(int by);";

fn program(source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(MODULE, source), MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn fingerprint(source: &str) -> u64 {
    program(source).fingerprint()
}

fn names(list: &[&str]) -> Vec<ComponentName> {
    list.iter().map(|name| ComponentName::new(*name)).collect()
}

fn guard() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

fn hit(by: i64) -> ScriptEvent {
    ScriptEvent::new(guard(), "Hit").with(ScriptValue::Int(by))
}

/// The registered `script_intake` data system.
fn intake() -> &'static DataSystemRegistration {
    khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_intake")
        .expect("a `script_intake` data system is registered")
}

/// A runtime wired the way the engine wires one for scripts: the scripting
/// world among the services, the two channels and the demand among the
/// resources.
struct Wired {
    runtime: Runtime,
    scripts: Arc<Mutex<ScriptRuntime>>,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    demand: Arc<Interest<ScriptReads>>,
}

impl Wired {
    /// The scripting world holding `installed` as `MODULE`, when given.
    fn new(installed: Option<&str>) -> Self {
        let mut scripts = ScriptRuntime::new();
        if let Some(source) = installed {
            scripts.add_program(MODULE, program(source));
        }
        let scripts = Arc::new(Mutex::new(scripts));
        let reloads = Channel::bounded(64, WhenFull::DropOldest);
        let events = Channel::bounded(64, WhenFull::DropOldest);
        let demand = Arc::new(Interest::<ScriptReads>::new());
        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(reloads.clone());
        runtime.resources.insert(events.clone());
        runtime.resources.insert(Arc::clone(&demand));
        Self {
            runtime,
            scripts,
            reloads,
            events,
            demand,
        }
    }

    /// `source` recompiled as `MODULE`, sent with nothing announced.
    fn reload(&self, source: &str) {
        self.reloads.send(ScriptReload {
            module: MODULE.to_owned(),
            program: Some(program(source)),
        });
    }

    /// `source` recompiled as `MODULE` and handed over as the hot-reload
    /// pump hands it: what it reads announced as `module:<name>`, then the
    /// program sent.
    fn reload_announced(&self, source: &str) {
        let compiled = program(source);
        self.demand.announce(
            ScriptReads::incoming(MODULE),
            compiled
                .reads
                .iter()
                .map(|read| ComponentName::new(read.as_str()))
                .collect::<Vec<_>>(),
        );
        self.reloads.send(ScriptReload {
            module: MODULE.to_owned(),
            program: Some(compiled),
        });
    }

    /// One run of the registered intake; `resumed` as the runner says.
    fn run(&self, resumed: bool) {
        (intake().run)(
            &mut World::new(),
            &self.runtime,
            &mut OutputDeck::new(),
            SystemContext {
                since: Tick(0),
                resumed,
            },
        );
    }

    /// The fingerprint of the program installed as `MODULE`.
    fn installed(&self) -> Option<u64> {
        self.scripts
            .lock()
            .expect("the scripting world")
            .program(MODULE)
            .map(|program| program.fingerprint())
    }

    fn pending(&self) -> usize {
        self.scripts
            .lock()
            .expect("the scripting world")
            .pending_len()
    }

    /// Leaves `count` hits in the mail, as a previous frame would.
    fn leave_mail(&self, count: usize) {
        let mut scripts = self.scripts.lock().expect("the scripting world");
        let mut pending = scripts.take_pending();
        for _ in 0..count {
            pending.push(hit(1));
        }
        scripts.set_pending(pending);
    }
}

// ─── Registration ──────────────────────────────────────────────────────────

/// **A `PreExtract` system of the scripting family, after the
/// declarations.** Before the flows project (they run after `PreExtract`),
/// after `script_components` — itself after the hot-reload pump — so a
/// reload's components exist when its program is installed; skipped while
/// the scripting family is off.
#[test]
fn the_intake_is_a_pre_extract_system_of_the_scripting_family_after_the_declarations() {
    let system = intake();

    assert_eq!(system.phase, TickPhase::PreExtract);
    assert_eq!(
        system.role,
        Role::Serves(khora_core::script::SCRIPT),
        "the scripting family's"
    );
    assert!(
        system.runs_after.contains(&"script_components"),
        "after the declarations: {:?}",
        system.runs_after
    );
}

// ─── Reloads ───────────────────────────────────────────────────────────────

/// **A reload whose reads the demand covers is installed by the intake** —
/// before any lane runs: the program is in the scripting world when the
/// intake returns, and the channel is empty.
#[test]
fn a_covered_reload_is_installed_by_the_intake() {
    let wired = Wired::new(Some(COUNTING));
    wired.reload_announced(READS_MARKER);

    wired.run(false);

    assert_eq!(
        wired.installed(),
        Some(fingerprint(READS_MARKER)),
        "the new program is installed"
    );
    assert!(wired.reloads.is_empty(), "the reload channel is drained");
}

/// **A reload whose reads the demand does not cover waits, and lands once
/// covered.** Nothing announced `Marker`, so the frame's projection would
/// not hold it: the old program stays. The intake announces the waiting
/// program's reads, so the next frame's projection holds them, and that
/// frame's intake installs it.
#[test]
fn an_uncovered_reload_waits_and_is_installed_once_covered() {
    let wired = Wired::new(Some(COUNTING));
    wired.reload(READS_MARKER);

    wired.run(false);

    assert_eq!(
        wired.installed(),
        Some(fingerprint(COUNTING)),
        "the reload waits: the old program stays"
    );
    assert!(wired.reloads.is_empty(), "taken out of the channel to wait");
    assert!(
        wired.demand.covers(&names(&["Marker"])),
        "the waiting program's reads are announced: {:?}",
        wired.demand.wanted()
    );

    wired.run(false);

    assert_eq!(
        wired.installed(),
        Some(fingerprint(READS_MARKER)),
        "covered now, it is installed"
    );
}

/// **A newer reload of a module replaces its waiting one.** The first edit
/// waits for `Marker`; the next frame `Marker` is covered, but a second edit
/// arrived — it alone counts, and waits for `Beacon`. The first edit never
/// lands, and what it read is no longer wanted.
#[test]
fn a_newer_reload_supersedes_a_waiting_one() {
    let wired = Wired::new(Some(COUNTING));
    wired.reload(READS_MARKER);
    wired.run(false);
    assert_eq!(
        wired.installed(),
        Some(fingerprint(COUNTING)),
        "the premise"
    );

    wired.reload(READS_BEACON);
    wired.run(false);

    assert_eq!(
        wired.installed(),
        Some(fingerprint(COUNTING)),
        "the replaced edit was not installed"
    );
    assert!(
        !wired.demand.covers(&names(&["Marker"])),
        "the replaced edit's reads left the demand: {:?}",
        wired.demand.wanted()
    );

    wired.run(false);

    assert_eq!(
        wired.installed(),
        Some(fingerprint(READS_BEACON)),
        "the newer edit lands"
    );
}

/// **A gone module is withdrawn, its entry with it.** A reload with no
/// program takes the module out of the scripting world, and what the pump
/// had announced for it is no longer wanted.
#[test]
fn a_gone_module_is_withdrawn_with_its_entry() {
    let wired = Wired::new(Some(COUNTING));
    wired
        .demand
        .announce(ScriptReads::incoming(MODULE), names(&["Marker"]));
    wired.reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: None,
    });

    wired.run(false);

    assert_eq!(wired.installed(), None, "the module left");
    assert!(
        wired.demand.wanted().is_empty(),
        "nothing is wanted for a gone module: {:?}",
        wired.demand.wanted()
    );
}

/// **After the intake, the demand holds what the installed programs read,
/// as `"loaded"`, and no entry of an installed module.** The pump announced
/// more than the program reads; once installed, only the program's own reads
/// are wanted, all of them under `"loaded"` — withdrawing that owner leaves
/// nothing.
#[test]
fn after_the_intake_the_installed_reads_are_wanted_as_loaded_alone() {
    let wired = Wired::new(None);
    wired
        .demand
        .announce(ScriptReads::incoming(MODULE), names(&["Marker", "Stale"]));
    wired.reload(READS_MARKER);

    wired.run(false);

    assert_eq!(
        wired.installed(),
        Some(fingerprint(READS_MARKER)),
        "the premise"
    );
    assert_eq!(wired.demand.wanted(), names(&["Marker"]));
    wired.demand.withdraw(ScriptReads::LOADED);
    assert!(
        wired.demand.wanted().is_empty(),
        "the module's own entry is withdrawn: {:?}",
        wired.demand.wanted()
    );
}

/// **A reload is taken in on the frame the scripting family resumes**, the
/// mail cleared beside it.
#[test]
fn a_reload_is_installed_on_the_frame_play_starts_over() {
    let wired = Wired::new(Some(COUNTING));
    wired.run(false);
    wired.reload_announced(READS_MARKER);

    wired.run(true);

    assert_eq!(wired.installed(), Some(fingerprint(READS_MARKER)));
}

/// **Without a demand, nothing waits**: nothing projects what scripts read,
/// so no reload could ever be covered.
#[test]
fn without_a_demand_every_reload_is_installed() {
    let mut wired = Wired::new(Some(COUNTING));
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::clone(&wired.scripts));
    runtime.resources.insert(wired.reloads.clone());
    runtime.resources.insert(wired.events.clone());
    wired.runtime = runtime;
    wired.reload(READS_MARKER);

    wired.run(false);

    assert_eq!(wired.installed(), Some(fingerprint(READS_MARKER)));
}

// ─── The mail ──────────────────────────────────────────────────────────────

/// **The engine's events join the mail.** Drained from the channel into the
/// scripting world's pending mail, after what was already waiting.
#[test]
fn engine_events_are_drained_into_the_mail() {
    let wired = Wired::new(Some(COUNTING));
    wired.leave_mail(1);
    wired.events.send(hit(2));
    wired.events.send(hit(3));

    wired.run(false);

    assert!(wired.events.is_empty(), "the event channel is drained");
    assert_eq!(wired.pending(), 3, "what waited, then the engine's two");
}

/// **Resuming clears the scripting world's own mail, and keeps the
/// channel's.** What scripts raised in the last session belongs to a world
/// that is gone; what is on the engine's channel was sent for this session —
/// the frame boundary emptied it while the family was off — so it joins the
/// mail.
#[test]
fn resuming_clears_the_runtimes_own_mail_and_keeps_the_channels() {
    let wired = Wired::new(Some(COUNTING));
    wired.run(false);
    wired.leave_mail(2);
    wired.events.send(hit(5));

    wired.run(true);

    assert_eq!(
        wired.pending(),
        1,
        "the last session's two are gone, the channel's one is kept"
    );
    assert!(wired.events.is_empty(), "the channel was drained, not left");
}

/// **The first session is no exception.** A first run is `resumed` too: the
/// scripting world's own mail is cleared on it as on any resumption, and the
/// channel's is kept — there is no "if a session already played".
#[test]
fn a_first_run_clears_the_runtimes_own_mail_like_any_resumption() {
    let wired = Wired::new(Some(COUNTING));
    wired.leave_mail(1);
    wired.events.send(hit(5));

    wired.run(true);

    assert_eq!(wired.pending(), 1, "the channel's hit alone");
}

/// **A frame that does not start play over keeps the mail**, however many
/// sessions played before.
#[test]
fn the_mail_is_kept_on_a_frame_that_does_not_resume() {
    let wired = Wired::new(Some(COUNTING));
    wired.run(false);
    wired.leave_mail(1);
    wired.events.send(hit(5));

    wired.run(false);

    assert_eq!(wired.pending(), 2);
}

/// **No scripting world, nothing taken**: an engine that registers none
/// keeps its channels' contents rather than losing them.
#[test]
fn without_a_scripting_world_the_channels_keep_their_contents() {
    let wired = Wired::new(None);
    let mut runtime = Runtime::default();
    runtime.resources.insert(wired.reloads.clone());
    runtime.resources.insert(wired.events.clone());
    runtime.resources.insert(Arc::clone(&wired.demand));
    wired.reload(READS_MARKER);
    wired.events.send(hit(1));

    (intake().run)(
        &mut World::new(),
        &runtime,
        &mut OutputDeck::new(),
        SystemContext {
            since: Tick(0),
            resumed: false,
        },
    );

    assert_eq!(wired.reloads.len(), 1);
    assert_eq!(wired.events.len(), 1);
}

// ─── `take_in`, the body over a locked scripting world ─────────────────────

/// **`take_in` drains both channels into the scripting world it is handed**:
/// the covered reload installed, the event in the mail.
#[test]
fn take_in_drains_both_channels_into_the_scripting_world() {
    let mut scripts = ScriptRuntime::new();
    scripts.add_program(MODULE, program(COUNTING));
    let reloads = Channel::bounded(8, WhenFull::DropOldest);
    let events = Channel::bounded(8, WhenFull::DropOldest);
    let demand = Interest::<ScriptReads>::new();
    demand.announce(ScriptReads::incoming(MODULE), names(&["Marker"]));
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: Some(program(READS_MARKER)),
    });
    events.send(hit(1));

    take_in(
        &mut scripts,
        Some(&reloads),
        Some(&events),
        Some(&demand),
        false,
    );

    assert!(reloads.is_empty() && events.is_empty());
    assert_eq!(
        scripts.program(MODULE).map(|program| program.fingerprint()),
        Some(fingerprint(READS_MARKER))
    );
    assert_eq!(scripts.pending_len(), 1);
    assert_eq!(demand.wanted(), names(&["Marker"]));
}

// ─── The lane leaves the channels alone ────────────────────────────────────

/// **The lane runs behaviors; it takes nothing in.** Handed the channels
/// all the same, a lane run leaves the reload and the event where they are,
/// and installs nothing.
#[test]
fn the_lane_leaves_the_engine_channels_alone() {
    let mut scripts = ScriptRuntime::new();
    scripts.add_program(MODULE, program(COUNTING));
    let reloads: Channel<ScriptReload> = Channel::bounded(8, WhenFull::DropOldest);
    let events: Channel<ScriptEvent> = Channel::bounded(8, WhenFull::DropOldest);
    let demand = Arc::new(Interest::<ScriptReads>::new());
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: Some(program(READS_MARKER)),
    });
    events.send(hit(1));
    let view = ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: guard(),
            program: 0,
            arrival: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
            row: Vec::new(),
        }],
    };

    let mut deck = OutputDeck::new();
    {
        let mut ctx = LaneContext::new();
        ctx.insert_ref(&view);
        ctx.insert(Fuel(u64::MAX));
        ctx.insert_slot(&mut scripts);
        ctx.insert_slot(&mut deck);
        ctx.insert_ref(&reloads);
        ctx.insert_ref(&events);
        ctx.insert_ref(&demand);
        BudgetedScriptLane::new()
            .execute(&mut ctx)
            .expect("the lane runs");
    }

    assert_eq!(reloads.len(), 1, "the reload is left for the intake");
    assert_eq!(events.len(), 1, "the event is left for the intake");
    assert_eq!(
        scripts.program(MODULE).map(|program| program.fingerprint()),
        Some(fingerprint(COUNTING)),
        "the lane installed nothing"
    );
    assert!(
        demand.wanted().is_empty(),
        "the lane announced nothing: {:?}",
        demand.wanted()
    );
}
