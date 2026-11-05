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

//! **A family limited to a marker runs, and is handed the marker's key.**
//! While φ is `Scoped(marker)` in the runtime's `Activation`, the runners run
//! a `Role::Serves(φ)` system and Flow — the family is on — and hand them
//! `scope: Some(key)`, the key the world registers `marker` under
//! (`World::components().key_named`). A family on unscoped, a runtime with no
//! `Activation`, and an `Upkeep` registration get `None`. A marker the world
//! does not know is handed as `ComponentKey::named(marker)`, the key a
//! component declared under that name would have: no entity holds it, so the
//! family works on none.

use std::sync::{Arc, Mutex};

use crate::substrate::{run_data_systems, run_flows};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Activation;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::ScriptValue;
use khora_core::Runtime;
use khora_data::ecs::{
    ComponentKey, ComponentProvenance, DataSystemRegistration, FieldKind, PackedField,
    PackedLayout, Role, RuntimeComponentDecl, ScriptRole, SemanticDomain, SystemContext, TickPhase,
    Transform, World,
};
use khora_data::flow::{Flow, FlowContext, ScriptView, Selection};

/// The family the probes work for.
const FAMILY: AgentId = AgentId::new("scope_probe_family");

/// The marker the probes' family is scoped to.
const MARKER: &str = "ScopeProbeMarker";

const SERVING: &str = "scope_probe_serving";
const UPKEEP: &str = "scope_probe_upkeep";
const FLOW: &str = "scope_probe_flow";
const PHASE: TickPhase = TickPhase::PreSimulation;

/// One run of a probe: on which world, which probe, the scope it was handed.
#[derive(Debug, Clone, Copy)]
struct ProbeRun {
    world: u64,
    probe: &'static str,
    scope: Option<ComponentKey>,
}

/// Every probe run of every test of this file — each test reads back its own
/// world's.
static RUNS: Mutex<Vec<ProbeRun>> = Mutex::new(Vec::new());

fn note(probe: &'static str, world: &World, scope: Option<ComponentKey>) {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(ProbeRun {
            world: world.instance_id(),
            probe,
            scope,
        });
}

fn serving(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    note(SERVING, world, ctx.scope);
}

fn upkeep(world: &mut World, _: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    note(UPKEEP, world, ctx.scope);
}

inventory::submit! {
    DataSystemRegistration {
        name: SERVING,
        phase: PHASE,
        run: serving,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Serves(FAMILY),
    }
}
inventory::submit! {
    DataSystemRegistration {
        name: UPKEEP,
        phase: PHASE,
        run: upkeep,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Upkeep,
    }
}

/// The view the probe flow publishes.
#[derive(Debug, Clone, PartialEq)]
struct ScopeProbeView;

/// Notes the scope the substrate handed it.
#[derive(Default)]
struct ScopeProbeFlow;

impl Flow for ScopeProbeFlow {
    type View = ScopeProbeView;
    const DOMAIN: SemanticDomain = SemanticDomain::Ui;
    const NAME: &'static str = FLOW;
    const ROLE: Role = Role::Serves(FAMILY);

    fn select(&mut self, world: &World, _runtime: &Runtime, ctx: FlowContext) -> Selection {
        note(FLOW, world, ctx.scope);
        Selection::new()
    }

    fn project(&self, _world: &World, _sel: &Selection, _runtime: &Runtime) -> ScopeProbeView {
        ScopeProbeView
    }
}

khora_data::register_flow!(ScopeProbeFlow);

/// The scopes `probe` was handed on `world`, in run order.
fn scopes_of(probe: &str, world: &World) -> Vec<Option<ComponentKey>> {
    RUNS.lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|run| run.world == world.instance_id() && run.probe == probe)
        .map(|run| run.scope)
        .collect()
}

/// A runtime holding an `Activation` that knows `families`, and the
/// activation.
fn runtime_with(families: &[AgentId]) -> (Runtime, Arc<Activation>) {
    let activation = Arc::new(Activation::new(families.iter().copied()));
    let mut runtime = Runtime::new();
    runtime.resources.insert(Arc::clone(&activation));
    (runtime, activation)
}

/// Registers the marker `name`: a declared component with no field, as an
/// Ergon `component Preview {}` is.
fn declare_marker(world: &mut World, name: &str) -> ComponentKey {
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: name.to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(Vec::new()).expect("an empty layout"),
        })
        .expect("the marker registers")
}

/// One frame of the substrate: the probes' phase, then the Flows.
fn frame(world: &mut World, runtime: &Runtime) -> LaneBus {
    run_data_systems(world, runtime, &mut OutputDeck::new(), PHASE);
    let mut bus = LaneBus::new();
    run_flows(world, &mut bus, runtime);
    bus
}

/// **A scoped family's system and Flow run, and are handed the marker's
/// key.** The upkeep system, in the same phase, is handed no scope.
#[test]
fn a_scoped_family_runs_and_is_handed_the_markers_key() {
    let mut world = World::new();
    let key = declare_marker(&mut world, MARKER);
    let (runtime, activation) = runtime_with(&[FAMILY]);
    activation.scope(FAMILY, MARKER).expect("a known family");
    activation.settle();

    frame(&mut world, &runtime);

    assert_eq!(
        scopes_of(SERVING, &world),
        vec![Some(key)],
        "the serving system runs, scoped"
    );
    assert_eq!(
        scopes_of(FLOW, &world),
        vec![Some(key)],
        "the serving Flow runs, scoped"
    );
    assert_eq!(
        scopes_of(UPKEEP, &world),
        vec![None],
        "upkeep is never scoped"
    );
}

/// **The scope follows the state in force.** No `Activation`: no scope. The
/// family on: no scope. Scoped: the key. Switched on again: no scope.
#[test]
fn the_scope_handed_follows_the_family_state_in_force() {
    let mut world = World::new();
    let key = declare_marker(&mut world, MARKER);

    frame(&mut world, &Runtime::new());
    let (runtime, activation) = runtime_with(&[FAMILY]);
    frame(&mut world, &runtime);
    activation.scope(FAMILY, MARKER).expect("a known family");
    frame(&mut world, &runtime);
    activation.settle();
    frame(&mut world, &runtime);
    activation.activate(FAMILY).expect("a known family");
    activation.settle();
    frame(&mut world, &runtime);

    let expected = vec![None, None, None, Some(key), None];
    assert_eq!(scopes_of(SERVING, &world), expected, "the system");
    assert_eq!(scopes_of(FLOW, &world), expected, "the Flow");
}

/// **A marker the world does not know scopes to the key of its name**, which
/// no entity holds: the family is on — its system and Flow run — and works on
/// nothing.
#[test]
fn a_marker_the_world_does_not_know_scopes_to_the_key_of_its_name() {
    let mut world = World::new();
    let (runtime, activation) = runtime_with(&[FAMILY]);
    activation
        .scope(FAMILY, "NoSuchScopeMarker")
        .expect("a known family");
    activation.settle();

    frame(&mut world, &runtime);

    let nothing = Some(ComponentKey::named("NoSuchScopeMarker"));
    assert_eq!(world.components().key_named("NoSuchScopeMarker"), None);
    assert_eq!(scopes_of(SERVING, &world), vec![nothing]);
    assert_eq!(scopes_of(FLOW, &world), vec![nothing]);
}

// ─── The scripts, through the substrate ─────────────────────────────────────

const SCRIPT: AgentId = khora_core::script::SCRIPT;

/// A world running `Guard` on two entities, the first marked `Preview`.
fn two_guards() -> (World, EntityId, EntityId) {
    let mut world = World::new();
    let guard = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Guard".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(vec![PackedField {
                name: "health".to_owned(),
                kind: FieldKind::Int,
                default: ScriptValue::Int(0),
            }])
            .expect("a valid layout"),
        })
        .expect("the behavior registers");
    world.set_script_role(
        guard,
        Some(ScriptRole::Behavior {
            module: "ai/guard.erg".to_owned(),
            order: 0,
        }),
    );
    world.set_declared_by(guard, Some("ai/guard.erg".to_owned()));
    let preview = declare_marker(&mut world, "Preview");
    let row = ScriptValue::Struct(Vec::new());
    let marked = world.spawn(Transform::identity());
    world
        .add_runtime_component(marked, guard, &row)
        .expect("attached");
    world
        .add_runtime_component(marked, preview, &row)
        .expect("marked");
    let outside = world.spawn(Transform::identity());
    world
        .add_runtime_component(outside, guard, &row)
        .expect("attached");
    (world, marked, outside)
}

/// The entities the published script view runs.
fn scripted(bus: &LaneBus) -> Vec<EntityId> {
    bus.get::<ScriptView>()
        .expect("the script flow publishes while the scripts are on")
        .instances
        .iter()
        .map(|instance| instance.entity)
        .collect()
}

/// **The scripts scoped to `Preview` run only the marked behavior**, on the
/// road a frame takes: the activation, the substrate, the script flow.
#[test]
fn scripts_scoped_to_a_marker_project_only_the_marked_behaviour() {
    let (mut world, marked, _outside) = two_guards();
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    activation.scope(SCRIPT, "Preview").expect("a known family");
    activation.settle();

    let bus = frame(&mut world, &runtime);

    assert_eq!(scripted(&bus), vec![marked]);
}

/// **Scripts scoped to a marker the world does not know run nothing**, on
/// the same road.
#[test]
fn scripts_scoped_to_an_unknown_marker_project_no_behaviour() {
    let (mut world, _marked, _outside) = two_guards();
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    activation
        .scope(SCRIPT, "NoSuchScopeMarker")
        .expect("a known family");
    activation.settle();

    let bus = frame(&mut world, &runtime);

    assert!(scripted(&bus).is_empty(), "{:?}", scripted(&bus));
}
