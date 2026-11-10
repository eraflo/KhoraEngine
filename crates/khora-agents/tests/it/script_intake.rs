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
//! boundary, while the scripting family is on.** The `script_intake` data
//! system runs after the declarations are registered at the frame's
//! `Boundary`, before the script flow projects — so a recompiled module the
//! `loading` family announced and staged is installed before the flow projects
//! what it reads, and its first `Get` finds the component. While the family
//! is off it is skipped: the reloads wait in the reserve, the engine's events
//! are dropped at each boundary; resuming clears the last session's own mail.
//!
//! Also the helper the harnesses that run the script agent without a
//! `PreExtract` boundary call before it: [`run_intake`].

use std::sync::{Arc, Mutex};

use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{Activation, Agent};
use khora_core::event::{discard_mail_of_inactive, Channel};
use khora_core::lane::{LaneBus, OutputDeck, Reserve, Staging};
use khora_core::projection::Interest;
use khora_core::script::{
    engine_event_channel, ComponentName, ScriptCatalog, ScriptEvent, ScriptReads, ScriptValue,
};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::{
    DataSystemRegistration, RigidBody, SystemContext, Tick, TickPhase, Transform, World,
};
use khora_data::flow::{Flow, FlowContext, ScriptFlow};
use khora_io::script::declarations::DeclaredComponents;
use khora_io::script::project_compiler::ProjectCompiler;
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::Value;

use super::behavior_session::attach;
use crate::script_family::fixtures::{publish_script_views, script_agent};

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
            scope: None,
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
/// the reserve, the event channel, the declarations, the catalog and the
/// demand among the resources; and, when the test switches the scripting
/// family, the engine's `Activation`.
struct Wiring {
    scripts: Arc<Mutex<ScriptRuntime>>,
    reserve: Reserve,
    events: Channel<ScriptEvent>,
    demand: Arc<Interest<ScriptReads>>,
    activation: Option<Arc<Activation>>,
    runtime: Arc<Runtime>,
}

impl Wiring {
    /// The wiring of a game that never switches its scripts off: no
    /// `Activation`, every family on.
    fn new() -> Self {
        Self::over(None)
    }

    /// The wiring of a tool that switches its scripts, starting with them
    /// `on` or off.
    fn switchable(on: bool) -> Self {
        let activation = Arc::new(Activation::new([khora_core::script::SCRIPT]));
        if !on {
            activation
                .deactivate(khora_core::script::SCRIPT)
                .expect("a known family");
        }
        Self::over(Some(activation))
    }

    fn over(activation: Option<Arc<Activation>>) -> Self {
        let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
        let reserve = Reserve::new();
        let events = engine_event_channel();
        let demand = Arc::new(Interest::<ScriptReads>::new());
        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(reserve.clone());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(events.clone());
        runtime.resources.insert(Arc::new(ScriptCatalog::default()));
        runtime.resources.insert(Arc::clone(&demand));
        if let Some(activation) = &activation {
            runtime.resources.insert(Arc::clone(activation));
        }
        Self {
            scripts,
            reserve,
            events,
            demand,
            activation,
            runtime: Arc::new(runtime),
        }
    }

    /// Requests the scripting family on or off, as an app's `before_frame`
    /// does; it counts from the next boundary.
    fn set_scripts(&self, on: bool) {
        let activation = self.activation.as_ref().expect("a switchable wiring");
        if on {
            activation.activate(khora_core::script::SCRIPT)
        } else {
            activation.deactivate(khora_core::script::SCRIPT)
        }
        .expect("a known family");
    }

    /// The frame boundary, as the SDK runs it — the requests settled, the
    /// mail of a family off for the frame dropped, the `Boundary` systems —
    /// then the frame's `PreExtract` systems over `world`.
    fn pre_extract(&self, world: &mut World) {
        if let Some(activation) = &self.activation {
            activation.settle();
            discard_mail_of_inactive(activation, &self.runtime);
        }
        run_data_systems(
            world,
            &self.runtime,
            &mut OutputDeck::new(),
            TickPhase::Boundary,
        );
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

    /// `HEARING` compiled as `ai/guard.erg`, staged with what it reads
    /// (nothing) announced, as the `loading` family stages it.
    fn send_the_guard(&self) {
        let result = khora_script::compile_module(
            &khora_script::MemoryLoader::new().with("ai/guard.erg", HEARING),
            "ai/guard.erg",
        );
        let program = result.program.expect("the guard compiles");
        self.demand
            .announce(ScriptReads::incoming("ai/guard.erg"), Vec::new());
        crate::staged_reloads::stage(
            &self.reserve,
            ScriptReload {
                module: "ai/guard.erg".to_owned(),
                program: Some(program),
            },
        );
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

// ─── A reload the compile handed over, end to end ──────────────────────────

/// **A module the project compiler handed over is installed at the boundary,
/// before the flow projects, and its first `Get` reads the component.** The
/// compile's route — what `mount` and a recompile take — announces what the
/// program reads as `module:<name>` and stages it. The frame's systems
/// install it: the program is in the scripting world before any lane runs.
/// The flow then projects what it reads, and the probe's very first `Get`
/// finds its rigid body rather than `null`.
#[test]
fn a_reload_the_compile_handed_over_is_installed_before_the_flow_and_reads_its_component_at_once() {
    let wiring = Wiring::new();
    let assets = tempfile::TempDir::new().expect("a temp dir");
    let scripts_dir = assets.path().join("scripts");
    std::fs::create_dir_all(scripts_dir.join("game")).expect("makes the script root");
    std::fs::write(scripts_dir.join(MODULE), READS_ITS_OWN_BODY).expect("writes");
    let compiled = ProjectCompiler::from_runtime(&wiring.runtime).compile_all(&scripts_dir);
    assert_eq!(compiled.reloads.len(), 1, "the premise: the probe compiled");
    let mut staging = Staging::new();
    staging.stage(compiled.reloads);
    staging.stage(compiled.declarations);
    wiring.reserve.receive(staging);
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
    assert!(
        wiring.reserve.is_empty::<ScriptReload>(),
        "the staged reload is taken"
    );
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
    let selection = flow.select(
        &world,
        &runtime,
        FlowContext {
            resumed: true,
            scope: None,
        },
    );
    let mut view = flow.project(&world, &selection, &runtime);
    view.delta_seconds = 0.016;
    let mut bus = LaneBus::new();
    publish_script_views(&mut bus, view);
    let mut agent = script_agent();
    crate::protocol::pin_with(
        &mut agent,
        StrategyId::Balanced,
        std::time::Duration::from_millis(50),
    );
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    {
        let mut ctx = EngineContext::for_agent(
            Arc::clone(&runtime),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        khora_agents::agent::run_frame(&mut agent, &mut ctx);
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

// ─── While the scripting family is on ──────────────────────────────────────

/// **Nothing is taken in while the scripts are off; a reload waits for them,
/// an event sent meanwhile is not delivered.** The boundary skips the intake:
/// the reload stays in the reserve, which holds it across frames,
/// nothing is installed; the engine's event is dropped at the boundary (its
/// mailbox drops between sessions). Once the scripts are on, the reload is
/// installed and the mail is empty.
#[test]
fn nothing_is_taken_in_while_the_scripts_are_off_and_reloads_wait_for_them() {
    let wiring = Wiring::switchable(false);
    let mut world = World::new();
    wiring.send_the_guard();
    wiring.events.send(a_hit());

    wiring.pre_extract(&mut world);
    wiring.pre_extract(&mut world);

    assert_eq!(
        wiring.reserve.len::<ScriptReload>(),
        1,
        "the reload waits for the scripts"
    );
    assert!(
        wiring.events.is_empty(),
        "the event sent while the scripts were off is dropped"
    );
    assert!(!wiring.installed("ai/guard.erg"));
    assert_eq!(wiring.pending(), 0);

    wiring.set_scripts(true);
    wiring.pre_extract(&mut world);

    assert!(wiring.installed("ai/guard.erg"), "installed once on");
    assert!(wiring.reserve.is_empty::<ScriptReload>());
    assert_eq!(
        wiring.pending(),
        0,
        "mail sent to the scripts while they were off is not delivered"
    );
}

/// **Resuming clears the last session's mail.** A session ran; the scripts
/// go off: the boundary skips the intake, and meanwhile the scripting world
/// is left mail and the engine raises an event, which the next boundary
/// that leaves the scripts off drops. The scripts back on: the intake
/// resumes, clears the scripting world's own mail, and none of it is kept.
#[test]
fn resuming_the_scripts_clears_the_mail_of_the_last_session() {
    let wiring = Wiring::switchable(true);
    let mut world = World::new();
    wiring.pre_extract(&mut world);

    wiring.set_scripts(false);
    wiring.pre_extract(&mut world);
    wiring.leave_mail(2);
    wiring.events.send(a_hit());
    wiring.pre_extract(&mut world);
    wiring.set_scripts(true);
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
    let wiring = Wiring::new();
    let mut world = World::new();
    wiring.pre_extract(&mut world);
    wiring.leave_mail(1);
    wiring.events.send(a_hit());

    wiring.pre_extract(&mut world);

    assert_eq!(wiring.pending(), 2);
}
