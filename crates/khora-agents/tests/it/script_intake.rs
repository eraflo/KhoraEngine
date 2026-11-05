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

//! **The scripting world takes in what the engine handed it at the frame
//! boundary, in play alone.** The `script_intake` data system runs in
//! `PreExtract` — after the hot-reload pump and the declarations, before the
//! script flow projects — so a recompiled module the pump announced is
//! installed before the flow projects what it reads, and its first `Get`
//! finds the component. While editing it is skipped and the channels keep
//! what they hold; play starting over clears the last session's mail.
//!
//! Also the helper the harnesses that run the script agent without a
//! `PreExtract` boundary call before it: [`run_intake`].

use std::sync::{Arc, Mutex, RwLock};

use khora_agents::script_agent::ScriptAgent;
use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::{Agent, EngineMode, SharedEngineMode};
use khora_core::event::Channel;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::projection::Interest;
use khora_core::script::{
    engine_event_channel, ComponentName, ScriptCatalog, ScriptEvent, ScriptReads, ScriptValue,
};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{
    DataSystemRegistration, RigidBody, SystemContext, Tick, TickPhase, Transform, World,
};
use khora_data::flow::{Flow, ScriptFlow};
use khora_io::script::declarations::{
    declaration_channel, DeclaredComponents, PendingDeclarations,
};
use khora_io::script::hot_reload::{load_all_announcing, reload_channel, PendingReloads};
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::Value;

use super::behavior_session::attach;

/// The registered `script_intake` data system.
fn intake() -> &'static DataSystemRegistration {
    khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_intake")
        .expect("a `script_intake` data system is registered")
}

/// Runs the `script_intake` data system once against `runtime`, as the
/// `PreExtract` boundary runs it on a frame of play that follows the last
/// one — what a harness running the script agent without that boundary
/// calls before the agent.
pub(crate) fn run_intake(runtime: &Runtime) {
    (intake().run)(
        &mut World::new(),
        runtime,
        &mut OutputDeck::new(),
        SystemContext {
            since: Tick(0),
            resumed: false,
        },
    );
}

const MODULE: &str = "game/probe.erg";

/// Reads its own rigid body every frame: the mass, `-2` when it reads
/// `null`, and the first value it ever read.
const READS_ITS_OWN_BODY: &str = "import \"engine/components.erg\";
behavior Probe {
    float mass = -1.0;
    float first = -1.0;
    void Update(float dt) {
        if (var b = this.Get(RigidBody)) { mass = b.mass; } else { mass = -2.0; }
        if (first == -1.0) { first = mass; }
    }
}";

/// A guard counting the hits it heard.
const HEARING: &str = "event Hit(int by);
behavior Guard {
    int hits = 0;
    on Hit(int by) { hits += by; }
}";

/// The engine's script resources, as `khora_sdk::scripts::install` and the
/// bootstrap put them in a runtime: the scripting world among the services;
/// the channels, the declarations, the catalog and the demand among the
/// resources; and the engine's mode.
struct Wiring {
    scripts: Arc<Mutex<ScriptRuntime>>,
    reloads: PendingReloads,
    declarations: PendingDeclarations,
    events: Channel<ScriptEvent>,
    demand: Arc<Interest<ScriptReads>>,
    mode: SharedEngineMode,
    runtime: Arc<Runtime>,
}

impl Wiring {
    fn new(mode: EngineMode) -> Self {
        let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
        let reloads = reload_channel();
        let declarations = declaration_channel();
        let events = engine_event_channel();
        let demand = Arc::new(Interest::<ScriptReads>::new());
        let mode: SharedEngineMode = Arc::new(RwLock::new(mode));
        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(reloads.clone());
        runtime.resources.insert(declarations.clone());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(events.clone());
        runtime.resources.insert(Arc::new(ScriptCatalog::default()));
        runtime.resources.insert(Arc::clone(&demand));
        runtime.resources.insert(Arc::clone(&mode));
        Self {
            scripts,
            reloads,
            declarations,
            events,
            demand,
            mode,
            runtime: Arc::new(runtime),
        }
    }

    fn set_mode(&self, mode: EngineMode) {
        *self.mode.write().expect("the mode lock") = mode;
    }

    /// The frame boundary's `PreExtract` systems over `world`.
    fn pre_extract(&self, world: &mut World) {
        run_data_systems(
            world,
            &self.runtime,
            &mut OutputDeck::new(),
            TickPhase::PreExtract,
        );
    }

    fn installed(&self, module: &str) -> bool {
        self.scripts
            .lock()
            .expect("the scripting world")
            .program(module)
            .is_some()
    }

    fn pending(&self) -> usize {
        self.scripts
            .lock()
            .expect("the scripting world")
            .pending_len()
    }

    /// Leaves `count` events in the mail, as a frame of the session would.
    fn leave_mail(&self, count: usize) {
        let mut scripts = self.scripts.lock().expect("the scripting world");
        let mut pending = scripts.take_pending();
        for _ in 0..count {
            pending.push(a_hit());
        }
        scripts.set_pending(pending);
    }

    /// `HEARING` compiled as `ai/guard.erg`, sent with what it reads (nothing)
    /// announced, as the pump sends it.
    fn send_the_guard(&self) {
        let result = khora_script::compile_module(
            &khora_script::MemoryLoader::new().with("ai/guard.erg", HEARING),
            "ai/guard.erg",
        );
        let program = result.program.expect("the guard compiles");
        self.demand
            .announce(ScriptReads::incoming("ai/guard.erg"), Vec::new());
        self.reloads.send(ScriptReload {
            module: "ai/guard.erg".to_owned(),
            program: Some(program),
        });
    }
}

fn a_hit() -> ScriptEvent {
    ScriptEvent::new(
        khora_core::ecs::entity::EntityId {
            index: 1,
            generation: 0,
        },
        "Hit",
    )
    .with(ScriptValue::Int(1))
}

// ─── A reload the pump handed over, end to end ─────────────────────────────

/// **A module the pump handed over is installed at the boundary, before the
/// flow projects, and its first `Get` reads the component.** The pump's
/// route — what `mount` and a recompile take — announces what the program
/// reads as `module:<name>` and sends it. The frame's `PreExtract` systems
/// install it: the program is in the scripting world before any lane runs.
/// The flow then projects what it reads, and the probe's very first `Get`
/// finds its rigid body rather than `null`.
#[test]
fn a_reload_the_pump_handed_over_is_installed_before_the_flow_and_reads_its_component_at_once() {
    let wiring = Wiring::new(EngineMode::Playing);
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let scripts_dir = assets.path().join("scripts");
    std::fs::create_dir_all(scripts_dir.join("game")).expect("makes the script root");
    std::fs::write(scripts_dir.join(MODULE), READS_ITS_OWN_BODY).expect("writes");
    let sent = load_all_announcing(
        &scripts_dir,
        &wiring.reloads,
        &wiring.declarations,
        &wiring.demand,
        None,
    );
    assert_eq!(sent, 1, "the premise: the probe compiled");
    assert!(
        wiring.demand.covers(&[ComponentName::new("RigidBody")]),
        "the premise: what it reads is announced"
    );

    let mut world = World::new();
    let probe = world.spawn((
        Transform::identity(),
        RigidBody {
            mass: 2.0,
            ..RigidBody::default()
        },
    ));
    wiring.pre_extract(&mut world);

    assert!(
        wiring.installed(MODULE),
        "the boundary installed the program before any lane ran"
    );
    assert!(wiring.reloads.is_empty(), "the reload channel is drained");
    assert!(
        wiring.demand.covers(&[ComponentName::new("RigidBody")]),
        "what it reads is still wanted, now as loaded: {:?}",
        wiring.demand.wanted()
    );

    // The next frame: the boundary, the flow, the agent's lane.
    attach(&mut world, probe, "Probe", &[]);
    let runtime = Arc::clone(&wiring.runtime);
    wiring.pre_extract(&mut world);
    let mut flow = ScriptFlow::default();
    let selection = flow.select(&world, &runtime);
    let mut view = flow.project(&world, &selection, &runtime);
    view.delta_seconds = 0.016;
    let mut bus = LaneBus::new();
    bus.publish(view);
    let mut agent = ScriptAgent::default();
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(50),
        memory_limit: None,
        extra_params: Default::default(),
    });
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    {
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(&runtime),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.execute(&mut ctx);
    }

    let scripts = wiring.scripts.lock().expect("the scripting world");
    let slot = scripts
        .program(MODULE)
        .and_then(|program| program.layout("Probe"))
        .and_then(|layout| layout.slot_of("first"))
        .expect("the probe's `first` field");
    let first = match scripts
        .peek(probe, "Probe")
        .and_then(|instance| instance.fields.get(slot).cloned())
    {
        Some(Persisted::Scalar(Value::Float(value))) => Some(value),
        _ => None,
    };
    assert_eq!(first, Some(2.0), "the first `Get` read the rigid body");
}

// ─── Play alone ────────────────────────────────────────────────────────────

/// **Nothing is taken in while editing.** The boundary skips the intake:
/// the reload and the event stay in their channels, nothing is installed,
/// the mail is empty. Once play starts, all of it is taken in.
#[test]
fn nothing_is_taken_in_while_editing_and_all_of_it_once_play_starts() {
    let wiring = Wiring::new(EngineMode::Custom("editor".to_owned()));
    let mut world = World::new();
    wiring.send_the_guard();
    wiring.events.send(a_hit());

    wiring.pre_extract(&mut world);
    wiring.pre_extract(&mut world);

    assert_eq!(wiring.reloads.len(), 1, "the reload waits for play");
    assert_eq!(wiring.events.len(), 1, "the event waits for play");
    assert!(!wiring.installed("ai/guard.erg"));
    assert_eq!(wiring.pending(), 0);

    wiring.set_mode(EngineMode::Playing);
    wiring.pre_extract(&mut world);

    assert!(wiring.installed("ai/guard.erg"), "installed once playing");
    assert!(wiring.reloads.is_empty() && wiring.events.is_empty());
    assert_eq!(
        wiring.pending(),
        1,
        "the first session keeps what was sent before it"
    );
}

/// **Play starting over clears the last session's mail.** A session played;
/// Stop: the boundary skips the intake while editing, and meanwhile mail is
/// left and the engine raises an event. Play again: the intake resumes, and
/// none of it is kept.
#[test]
fn a_play_restart_clears_the_mail_of_the_last_session() {
    let wiring = Wiring::new(EngineMode::Playing);
    let mut world = World::new();
    wiring.pre_extract(&mut world);

    wiring.set_mode(EngineMode::Custom("editor".to_owned()));
    wiring.pre_extract(&mut world);
    wiring.leave_mail(2);
    wiring.events.send(a_hit());
    wiring.set_mode(EngineMode::Playing);
    wiring.pre_extract(&mut world);

    assert_eq!(wiring.pending(), 0, "the old session's mail is gone");
    assert!(
        wiring.events.is_empty(),
        "the backlog was drained, not left"
    );
}

/// **A frame of play after another keeps the mail** — only a restart
/// clears it.
#[test]
fn consecutive_frames_of_play_keep_the_mail() {
    let wiring = Wiring::new(EngineMode::Playing);
    let mut world = World::new();
    wiring.pre_extract(&mut world);
    wiring.leave_mail(1);
    wiring.events.send(a_hit());

    wiring.pre_extract(&mut world);

    assert_eq!(wiring.pending(), 2);
}
