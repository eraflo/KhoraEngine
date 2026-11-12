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
//! The declarations travel the way the engine sends them — staged in the
//! reserve by the `loading` family, beside the program — and are registered by
//! the `Boundary` data system, in every mode: an editor editing a scene, with
//! no script running, knows `Health` too. Then a
//! behavior writes and reads the component frame after frame, through the
//! flow, the lane and the applier.

use std::sync::{Arc, Mutex};

use khora_agents::agent::LaneAgent;
use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::Agent;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck, Reserve};
use khora_core::script::{engine_event_channel, ScriptCatalog, ScriptValue};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::{DataSystemRegistration, Role, SemanticDomain, TickPhase, Transform, World};
use khora_data::flow::{Flow, FlowContext, ScriptFlow};
use khora_io::script::declarations::{DeclaredComponents, ScriptDeclarations};
use khora_io::script::mirror::PreludeLoader;
use khora_lanes::script::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

use super::behavior_session::{attach, declared_in};
use crate::script_family::fixtures::{publish_script_views, script_agent};

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
    declared_in(HEALTH_MODULE, &compile(HEALTH_MODULE))
}

/// The resources the registration reads: the reserve the `loading` family
/// stages into, and the state the registration keeps between frames.
fn registration_runtime(declarations: &Reserve) -> Runtime {
    let mut runtime = Runtime::default();
    runtime.resources.insert(declarations.clone());
    runtime
        .resources
        .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
    runtime
}

// ─── Registration, in every mode ────────────────────────────────────────────

/// **The registration is an upkeep system of the frame's `Boundary`**, so a
/// reload's components exist before the intake installs the program compiled
/// with them, and are registered while the scripts are off too.
#[test]
fn script_components_register_at_the_frame_boundary_as_upkeep() {
    let system = khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "script_components")
        .expect("a `script_components` data system is registered");

    assert_eq!(system.phase, TickPhase::Boundary);
    assert_eq!(system.role, Role::Upkeep, "in every mode");
}

/// **An editor editing a scene knows `Health`.** No script agent, no lane,
/// nothing running gameplay: the boundary's data systems alone register what
/// the `loading` family staged.
#[test]
fn script_components_register_while_editing() {
    let declarations = Reserve::new();
    let runtime = registration_runtime(&declarations);
    let mut world = World::new();
    crate::staged_reloads::stage(&declarations, health_declarations());

    run_data_systems(
        &mut world,
        &runtime,
        &mut OutputDeck::new(),
        TickPhase::Boundary,
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
    assert!(
        declarations.is_empty::<ScriptDeclarations>(),
        "the staged declarations are taken"
    );

    // The next frame has nothing to register, and keeps what it registered.
    run_data_systems(
        &mut world,
        &runtime,
        &mut OutputDeck::new(),
        TickPhase::Boundary,
    );
    assert_eq!(world.components().key_named("Health"), Some(health));
}

// ─── Played ─────────────────────────────────────────────────────────────────

/// One running session, wired as the engine wires one: the reserve the
/// `loading` family stages into, the catalog, the demand on the script flow, the
/// scripting world.
struct Session {
    world: World,
    flow: ScriptFlow,
    /// Whether the flow ran before, as the substrate keeps it: its first run
    /// is a resumption.
    projected: bool,
    agent: LaneAgent,
    scripts: Arc<Mutex<ScriptRuntime>>,
    runtime: Arc<Runtime>,
}

impl Session {
    fn new(world: World) -> (Self, Reserve) {
        let scripts = Arc::new(Mutex::new(ScriptRuntime::new()));
        let reserve = Reserve::new();

        let mut runtime = registration_runtime(&reserve);
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(engine_event_channel());
        runtime.resources.insert(Arc::new(ScriptCatalog::default()));
        runtime
            .resources
            .insert(Arc::new(khora_core::projection::Interest::<
                khora_core::script::ScriptReads,
            >::new()));

        let mut agent = script_agent();
        crate::protocol::pin_with(
            &mut agent,
            StrategyId::Balanced,
            std::time::Duration::from_millis(50),
        );

        let session = Self {
            world,
            flow: ScriptFlow::default(),
            projected: false,
            agent,
            scripts,
            runtime: Arc::new(runtime),
        };
        (session, reserve)
    }

    /// The frame's `Boundary` systems: what was declared is registered.
    fn register(&mut self) {
        run_data_systems(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            TickPhase::Boundary,
        );
    }

    /// One frame: the `Boundary` and `PreExtract` systems, the projection,
    /// the scripts, then `Maintenance` applying what they queued.
    fn frame(&mut self) {
        self.register();
        run_data_systems(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            TickPhase::PreExtract,
        );
        let ctx = FlowContext {
            resumed: !self.projected,
            scope: None,
        };
        self.projected = true;
        let selection = self.flow.select(&self.world, &self.runtime, ctx);
        let mut view = self.flow.project(&self.world, &selection, &self.runtime);
        view.delta_seconds = 0.016;
        let mut bus = LaneBus::new();
        publish_script_views(&mut bus, view);
        let mut deck = OutputDeck::new();
        let permit = self.agent.contention();
        {
            let mut ctx = EngineContext::for_agent(
                Arc::clone(&self.runtime),
                &bus,
                &mut deck,
                &permit,
                Some(self.agent.id()),
            );
            khora_agents::agent::run_frame(&mut self.agent, &mut ctx);
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
/// component is registered** — both staged before the same frame, as a mount
/// or a hot reload stages them: the behavior adds `Health`, reads its own write,
/// and the `World` holds the row with the declared default for the field it
/// did not name. The medic is attached by its own component, registered
/// beforehand.
#[test]
fn a_behavior_adds_and_reads_a_component_its_project_declares() {
    let mut world = World::new();
    let medic = world.spawn(Transform::identity());
    let (mut session, reserve) = Session::new(world);
    crate::staged_reloads::stage(&reserve, declared_in(MEDIC_MODULE, &compile(MEDIC_MODULE)));
    session.register();
    attach(&mut session.world, medic, "Medic", &[]);
    assert!(
        session.world.components().key_named("Health").is_none(),
        "the premise: `Health` is not registered yet"
    );

    crate::staged_reloads::stage(&reserve, health_declarations());
    crate::staged_reloads::stage(
        &reserve,
        ScriptReload {
            module: MEDIC_MODULE.to_owned(),
            program: Some(compile(MEDIC_MODULE)),
        },
    );

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
