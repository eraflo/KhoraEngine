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

//! A project's own components, end to end (Ergon spec 08).
//!
//! The declarations travel the way the engine sends them — on the
//! declarations channel, beside the program on the reload channel — and are
//! registered by the `PreExtract` data system, in every mode: an editor
//! editing a scene, with no script running, knows `Health` too. Then a
//! behavior writes and reads the component frame after frame, through the
//! flow, the lane and the applier.

use std::sync::{Arc, Mutex};

use khora_agents::script_agent::ScriptAgent;
use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::event::Channel;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::{engine_event_channel, ScriptCatalog, ScriptValue};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{
    DataSystemRegistration, Script, SemanticDomain, TickPhase, Transform, World,
};
use khora_data::flow::{Flow, ScriptFlow};
use khora_io::script::declarations::{declaration_channel, DeclaredComponents, ScriptDeclarations};
use khora_io::script::mirror::PreludeLoader;
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const HEALTH_MODULE: &str = "game/health.erg";
const MEDIC_MODULE: &str = "game/medic.erg";

const HEALTH: &str = "[Domain(Physics)]
component Health { int current = 100; int max = 100; }";

/// Adds a `Health` once, then reads it every frame: `seen` is the `current`
/// it read, `-1` before it read one.
const MEDIC: &str = "import \"engine/components.erg\";
import \"game/health.erg\";
behavior Medic {
    int seen = -1;
    void Update(float dt) {
        if (!this.Has(Health)) { this.Add(Health { current: 7 }); }
        if (var h = this.Get(Health)) { seen = h.current; }
    }
}";

/// `module` compiled among the project's modules, the mirrors available.
fn compile(module: &str) -> Program {
    let loader = PreludeLoader::new(
        MemoryLoader::new()
            .with(HEALTH_MODULE, HEALTH)
            .with(MEDIC_MODULE, MEDIC),
    );
    let result = compile_module(&loader, module);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{module}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// What the engine sends for `Health`'s module.
fn health_declarations() -> ScriptDeclarations {
    ScriptDeclarations {
        module: HEALTH_MODULE.to_owned(),
        components: compile(HEALTH_MODULE)
            .declared
            .into_iter()
            .filter(|declared| declared.module == HEALTH_MODULE)
            .collect(),
    }
}

/// The resources the registration reads: the channel the compile path sends
/// on, and the state the registration keeps between frames.
fn registration_runtime(declarations: &Channel<ScriptDeclarations>) -> Runtime {
    let mut runtime = Runtime::default();
    runtime.resources.insert(declarations.clone());
    runtime
        .resources
        .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
    runtime
}

// ─── Registration, in every mode ────────────────────────────────────────────

/// **The registration is a `PreExtract` data system after the hot-reload
/// pump**, so a reload's components exist before the lane installs the
/// program compiled with them.
#[test]
fn script_components_register_at_the_frame_boundary_after_the_pump() {
    let system = khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_components")
        .expect("a `script_components` data system is registered");

    assert_eq!(system.phase, TickPhase::PreExtract);
    assert!(
        system.runs_after.contains(&"script_hot_reload"),
        "runs after the pump: {:?}",
        system.runs_after
    );
}

/// **An editor editing a scene knows `Health`.** No script agent, no lane,
/// nothing running gameplay: the boundary's data systems alone register what
/// the compile path sent.
#[test]
fn script_components_register_while_editing() {
    let declarations = declaration_channel();
    let runtime = registration_runtime(&declarations);
    let mut world = World::new();
    declarations.send(health_declarations());

    run_data_systems(
        &mut world,
        &runtime,
        &mut OutputDeck::new(),
        TickPhase::PreExtract,
    );

    let health = world
        .components()
        .key_named("Health")
        .expect("Health is registered");
    assert_eq!(
        world
            .components()
            .vtable(health)
            .map(|vtable| vtable.domain),
        Some(SemanticDomain::Physics)
    );
    assert!(declarations.is_empty(), "the channel is drained");

    // The next frame has nothing to register, and keeps what it registered.
    run_data_systems(
        &mut world,
        &runtime,
        &mut OutputDeck::new(),
        TickPhase::PreExtract,
    );
    assert_eq!(world.components().key_named("Health"), Some(health));
}

// ─── Played ─────────────────────────────────────────────────────────────────

/// One running session, wired as the engine wires one: the reload and
/// declaration channels, the catalog, the scripting world.
struct Session {
    world: World,
    flow: ScriptFlow,
    agent: ScriptAgent,
    scripts: Arc<Mutex<ScriptRuntime>>,
    runtime: Arc<Runtime>,
}

impl Session {
    fn new(world: World) -> (Self, Channel<ScriptReload>, Channel<ScriptDeclarations>) {
        let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
        let reloads = khora_io::script::hot_reload::reload_channel();
        let declarations = declaration_channel();

        let mut runtime = registration_runtime(&declarations);
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(reloads.clone());
        runtime.resources.insert(engine_event_channel());
        runtime.resources.insert(Arc::new(ScriptCatalog::default()));

        let mut agent = ScriptAgent::default();
        agent.apply_budget(ResourceBudget {
            strategy_id: StrategyId::Balanced,
            time_limit: std::time::Duration::from_millis(50),
            memory_limit: None,
            extra_params: Default::default(),
        });

        let session = Self {
            world,
            flow: ScriptFlow::default(),
            agent,
            scripts,
            runtime: Arc::new(runtime),
        };
        (session, reloads, declarations)
    }

    /// One frame: the boundary's `PreExtract` systems, the projection, the
    /// scripts, then `Maintenance` applying what they queued.
    fn frame(&mut self) {
        run_data_systems(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            TickPhase::PreExtract,
        );
        let selection = self.flow.select(&self.world, &self.runtime);
        let mut view = self.flow.project(&self.world, &selection, &self.runtime);
        view.delta_seconds = 0.016;
        let mut bus = LaneBus::new();
        bus.publish(view);
        let mut deck = OutputDeck::new();
        let permit = self.agent.contention();
        {
            let mut ctx = EngineContext::for_agent(
                WorldAccess::None,
                Arc::clone(&self.runtime),
                &bus,
                &mut deck,
                &permit,
                Some(self.agent.id()),
            );
            self.agent.execute(&mut ctx);
        }
        run_data_systems(
            &mut self.world,
            &self.runtime,
            &mut deck,
            TickPhase::Maintenance,
        );
    }

    fn seen(&self, entity: EntityId) -> Option<i64> {
        let scripts = self.scripts.lock().expect("the scripting world");
        let slot = scripts
            .program(MEDIC_MODULE)?
            .layout("Medic")?
            .slot_of("seen")?;
        match scripts.peek(entity, "Medic")?.fields.get(slot)? {
            Persisted::Scalar(Value::Int(seen)) => Some(*seen),
            _ => None,
        }
    }
}

/// **A program using a component its project declares runs once the
/// component is registered** — both sent in the same frame, as a mount or a
/// hot reload sends them: the behavior adds `Health`, reads its own write,
/// and the `World` holds the row with the declared default for the field it
/// did not name.
#[test]
fn a_behavior_adds_and_reads_a_component_its_project_declares() {
    let mut world = World::new();
    let medic = world.spawn((Transform::identity(), Script::new(MEDIC_MODULE, "Medic")));
    let (mut session, reloads, declarations) = Session::new(world);

    declarations.send(health_declarations());
    reloads.send(ScriptReload {
        module: MEDIC_MODULE.to_owned(),
        program: compile(MEDIC_MODULE),
    });

    let mut frames = 0;
    while session.seen(medic) != Some(7) {
        frames += 1;
        assert!(
            frames <= 6,
            "after {frames} frames the medic read {:?}",
            session.seen(medic)
        );
        session.frame();
    }

    let world = &session.world;
    let health = world
        .components()
        .key_named("Health")
        .expect("Health is registered");
    let packed = world
        .components()
        .vtable(health)
        .and_then(|vtable| vtable.columns.packed())
        .expect("a declared component");
    let row = world.row(medic, health).expect("the medic holds Health");
    assert_eq!(
        row.field(packed.slot_of("current").expect("current")),
        Some(ScriptValue::Int(7))
    );
    assert_eq!(
        row.field(packed.slot_of("max").expect("max")),
        Some(ScriptValue::Int(100)),
        "the field the `Add` did not name holds the declared default"
    );
}
