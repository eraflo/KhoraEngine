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

//! A family limited to a marker: its Flows project only the entities holding
//! the marker the substrate hands them in `FlowContext::scope`.
//!
//! For the scripts, an entity entering the scope arrives, as if it had just
//! appeared; one leaving it is forgotten, so it arrives afresh if it comes
//! back. An entity already projected when its family becomes scoped carries
//! on. A marker the world does not know is handed as the key a component
//! declared under that name would have (`ComponentKey::named`): no entity
//! holds it, so the family works on none.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::LaneBus;
use khora_core::script::ScriptValue;
use khora_core::Runtime;
use khora_data::ecs::{
    AudioSource, ComponentKey, ComponentProvenance, FieldKind, GlobalTransform, PackedField,
    PackedLayout, RuntimeComponentDecl, ScriptRole, SemanticDomain, Transform, World,
};
use khora_data::flow::{
    run_flow_cached, AudioFlow, AudioView, Flow, FlowContext, ScriptArrival, ScriptFlow, ScriptView,
};

// ─── Worlds ─────────────────────────────────────────────────────────────────

/// Registers the behavior `name` that `module` declares, with an `int` field
/// `health` of default 0, as the scripts' registration does.
fn declare_behavior(world: &mut World, module: &str, name: &str) -> ComponentKey {
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: name.to_owned(),
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
        key,
        Some(ScriptRole::Behavior {
            module: module.to_owned(),
            order: 0,
        }),
    );
    world.set_declared_by(key, Some(module.to_owned()));
    key
}

/// Registers a marker — a declared component with no field, as an Ergon
/// `component Preview {}` is.
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

/// Attaches the behavior `key` to `entity`, its `health` set to `health`.
fn attach_behavior(world: &mut World, entity: EntityId, key: ComponentKey, health: i64) {
    world
        .add_runtime_component(
            entity,
            key,
            &ScriptValue::Struct(vec![("health".to_owned(), ScriptValue::Int(health))]),
        )
        .expect("the behavior attaches");
}

/// Gives `entity` the marker `key`.
fn mark(world: &mut World, entity: EntityId, key: ComponentKey) {
    world
        .add_runtime_component(entity, key, &ScriptValue::Struct(Vec::new()))
        .expect("the marker attaches");
}

/// Takes the marker `key` from `entity`.
fn unmark(world: &mut World, entity: EntityId, key: ComponentKey) {
    assert!(
        world.remove_component_by_key(entity, key),
        "the entity held the marker"
    );
}

/// What the substrate hands a Flow of a family scoped to `scope`.
fn context(resumed: bool, scope: Option<ComponentKey>) -> FlowContext {
    FlowContext { resumed, scope }
}

/// A marker name no world here knows, as the substrate hands it.
fn unknown_marker() -> ComponentKey {
    ComponentKey::named("NoSuchMarker")
}

// ─── Scripts ────────────────────────────────────────────────────────────────

/// One frame of the script flow, as the substrate runs it.
fn script_frame(flow: &mut ScriptFlow, world: &World, ctx: FlowContext) -> ScriptView {
    let runtime = Runtime::default();
    let selection = flow.select(world, &runtime, ctx);
    flow.project(world, &selection, &runtime)
}

/// The entities a script view runs, in its order.
fn instances(view: &ScriptView) -> Vec<EntityId> {
    view.instances
        .iter()
        .map(|instance| instance.entity)
        .collect()
}

/// What `entity` brings this frame: `None` when it is not in the view,
/// `Some(None)` when it carries on, `Some(Some(_))` when it arrives.
fn arrival_of(view: &ScriptView, entity: EntityId) -> Option<Option<ScriptArrival>> {
    view.instances
        .iter()
        .find(|instance| instance.entity == entity)
        .map(|instance| instance.arrival.clone())
}

/// What an entity holding `Guard` at `health` arrives with.
fn guard_arrival(health: i64) -> ScriptArrival {
    ScriptArrival {
        fields: vec![("health".to_owned(), ScriptValue::Int(health))],
        observed: None,
    }
}

/// **A scoped script flow projects only the marked behaviors.** Three guards,
/// two of them marked: the view runs the two, and not the third.
#[test]
fn a_scoped_script_flow_projects_only_the_marked_behaviours() {
    let mut world = World::new();
    let guard = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let preview = declare_marker(&mut world, "Preview");
    let marked = world.spawn(Transform::identity());
    attach_behavior(&mut world, marked, guard, 100);
    mark(&mut world, marked, preview);
    let outside = world.spawn(Transform::identity());
    attach_behavior(&mut world, outside, guard, 100);
    let also_marked = world.spawn(Transform::identity());
    attach_behavior(&mut world, also_marked, guard, 100);
    mark(&mut world, also_marked, preview);

    let view = script_frame(
        &mut ScriptFlow::default(),
        &world,
        context(true, Some(preview)),
    );

    assert_eq!(
        instances(&view),
        vec![marked, also_marked],
        "only the marked guards run"
    );
}

/// **A scope the world does not know runs no behavior**: no entity holds the
/// key of an undeclared name.
#[test]
fn a_script_flow_scoped_to_an_unknown_marker_projects_no_behaviour() {
    let mut world = World::new();
    let guard = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let entity = world.spawn(Transform::identity());
    attach_behavior(&mut world, entity, guard, 100);

    let view = script_frame(
        &mut ScriptFlow::default(),
        &world,
        context(true, Some(unknown_marker())),
    );

    assert!(
        view.is_empty(),
        "an unknown marker scopes to nothing: {:?}",
        instances(&view)
    );
}

/// **An entity entering the scope arrives.** Unmarked, it is not run; given
/// the marker, it arrives with its authored fields, then carries on.
#[test]
fn an_entity_entering_the_scope_arrives() {
    let mut world = World::new();
    let guard = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let preview = declare_marker(&mut world, "Preview");
    let entity = world.spawn(Transform::identity());
    attach_behavior(&mut world, entity, guard, 80);
    let mut flow = ScriptFlow::default();

    let before = script_frame(&mut flow, &world, context(true, Some(preview)));
    assert_eq!(arrival_of(&before, entity), None, "outside the scope");

    mark(&mut world, entity, preview);
    let entered = script_frame(&mut flow, &world, context(false, Some(preview)));
    assert_eq!(
        arrival_of(&entered, entity),
        Some(Some(guard_arrival(80))),
        "it arrives as it enters the scope"
    );

    let after = script_frame(&mut flow, &world, context(false, Some(preview)));
    assert_eq!(arrival_of(&after, entity), Some(None), "then carries on");
}

/// **An entity leaving the scope is forgotten**: it is not run while it is
/// outside, and arrives afresh when it comes back.
#[test]
fn an_entity_leaving_the_scope_is_forgotten_and_arrives_afresh_when_it_comes_back() {
    let mut world = World::new();
    let guard = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let preview = declare_marker(&mut world, "Preview");
    let entity = world.spawn(Transform::identity());
    attach_behavior(&mut world, entity, guard, 60);
    mark(&mut world, entity, preview);
    let mut flow = ScriptFlow::default();
    let scoped = |resumed| context(resumed, Some(preview));

    let first = script_frame(&mut flow, &world, scoped(true));
    assert_eq!(arrival_of(&first, entity), Some(Some(guard_arrival(60))));
    let second = script_frame(&mut flow, &world, scoped(false));
    assert_eq!(arrival_of(&second, entity), Some(None));

    unmark(&mut world, entity, preview);
    let left = script_frame(&mut flow, &world, scoped(false));
    assert_eq!(
        arrival_of(&left, entity),
        None,
        "outside the scope, not run"
    );

    mark(&mut world, entity, preview);
    let back = script_frame(&mut flow, &world, scoped(false));
    assert_eq!(
        arrival_of(&back, entity),
        Some(Some(guard_arrival(60))),
        "forgotten when it left, it arrives afresh"
    );
}

/// **A scope moved to another marker brings that marker's entities in as
/// arrivals**, and forgets the first marker's, which arrive afresh when the
/// scope moves back.
#[test]
fn a_scope_moved_to_another_marker_brings_its_entities_in_as_arrivals() {
    let mut world = World::new();
    let guard = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let preview = declare_marker(&mut world, "Preview");
    let inspected = declare_marker(&mut world, "Inspected");
    let previewed = world.spawn(Transform::identity());
    attach_behavior(&mut world, previewed, guard, 10);
    mark(&mut world, previewed, preview);
    let watched = world.spawn(Transform::identity());
    attach_behavior(&mut world, watched, guard, 20);
    mark(&mut world, watched, inspected);
    let mut flow = ScriptFlow::default();

    let first = script_frame(&mut flow, &world, context(true, Some(preview)));
    assert_eq!(instances(&first), vec![previewed]);
    script_frame(&mut flow, &world, context(false, Some(preview)));

    let moved = script_frame(&mut flow, &world, context(false, Some(inspected)));
    assert_eq!(
        instances(&moved),
        vec![watched],
        "the other marker's entity"
    );
    assert_eq!(
        arrival_of(&moved, watched),
        Some(Some(guard_arrival(20))),
        "it arrives as the scope reaches it"
    );

    let back = script_frame(&mut flow, &world, context(false, Some(preview)));
    assert_eq!(
        arrival_of(&back, previewed),
        Some(Some(guard_arrival(10))),
        "forgotten when the scope left it, it arrives afresh"
    );
}

/// **An entity already run carries on when its family becomes scoped to a
/// marker it holds**: it never left the family's work, so it does not arrive
/// again; the unmarked one is no longer run.
#[test]
fn a_marked_entity_carries_on_when_its_family_becomes_scoped() {
    let mut world = World::new();
    let guard = declare_behavior(&mut world, "ai/guard.erg", "Guard");
    let preview = declare_marker(&mut world, "Preview");
    let marked = world.spawn(Transform::identity());
    attach_behavior(&mut world, marked, guard, 100);
    mark(&mut world, marked, preview);
    let outside = world.spawn(Transform::identity());
    attach_behavior(&mut world, outside, guard, 100);
    let mut flow = ScriptFlow::default();

    let unscoped = script_frame(&mut flow, &world, context(true, None));
    assert_eq!(instances(&unscoped), vec![marked, outside], "on: both run");

    let scoped = script_frame(&mut flow, &world, context(false, Some(preview)));
    assert_eq!(instances(&scoped), vec![marked], "scoped: only the marked");
    assert_eq!(
        arrival_of(&scoped, marked),
        Some(None),
        "the marked entity carries on"
    );
}

// ─── Audio ──────────────────────────────────────────────────────────────────

/// A world holding two sources, the first marked `Preview`, and the marker.
fn two_sources() -> (World, EntityId, EntityId, ComponentKey) {
    let mut world = World::new();
    let preview = declare_marker(&mut world, "Preview");
    let marked = world.spawn((GlobalTransform::identity(), AudioSource::default()));
    mark(&mut world, marked, preview);
    let outside = world.spawn((GlobalTransform::identity(), AudioSource::default()));
    (world, marked, outside, preview)
}

/// The sources of an audio view, in its order.
fn sources(view: &AudioView) -> Vec<EntityId> {
    view.sources.iter().map(|source| source.entity).collect()
}

/// One run of the audio flow, as the substrate runs it.
fn audio_frame(world: &World, ctx: FlowContext) -> AudioView {
    let mut flow = AudioFlow;
    let runtime = Runtime::new();
    let selection = flow.select(world, &runtime, ctx);
    flow.project(world, &selection, &runtime)
}

/// **A scoped audio flow projects only the marked sources** — what a sound
/// preview plays.
#[test]
fn a_scoped_audio_flow_projects_only_the_marked_sources() {
    let (world, marked, _outside, preview) = two_sources();

    let view = audio_frame(&world, context(true, Some(preview)));

    assert_eq!(sources(&view), vec![marked]);
}

/// **An audio flow scoped to an unknown marker projects no source.**
#[test]
fn an_audio_flow_scoped_to_an_unknown_marker_projects_no_source() {
    let (world, _marked, _outside, _preview) = two_sources();

    let view = audio_frame(&world, context(true, Some(unknown_marker())));

    assert!(view.sources.is_empty(), "{:?}", sources(&view));
}

/// **A cached audio view is never served across a change of scope.** The
/// world does not change between the runs: only the scope does, and each run
/// publishes the sources of its own scope.
#[test]
fn a_cached_audio_view_is_not_served_across_a_change_of_scope() {
    let (mut world, marked, outside, preview) = two_sources();
    let runtime = Runtime::new();
    let mut flow = AudioFlow;
    let mut cache = None;
    let mut run = |world: &mut World, scope| {
        let mut bus = LaneBus::new();
        run_flow_cached(
            &mut flow,
            &mut cache,
            world,
            &mut bus,
            &runtime,
            context(false, scope),
        );
        sources(bus.get::<AudioView>().expect("the audio flow publishes"))
    };

    assert_eq!(run(&mut world, None), vec![marked, outside], "unscoped");
    assert_eq!(run(&mut world, Some(preview)), vec![marked], "scoped");
    assert_eq!(
        run(&mut world, None),
        vec![marked, outside],
        "unscoped again"
    );
}

/// **A cached audio view follows the marker moving between sources.** The
/// scope stays the same; only which sources hold the marker changes, and the
/// next run publishes the sources marked now.
#[test]
fn a_cached_audio_view_follows_the_marker_moving_between_sources() {
    let (mut world, marked, outside, preview) = two_sources();
    let runtime = Runtime::new();
    let mut flow = AudioFlow;
    let mut cache = None;
    let mut run = |world: &mut World| {
        let mut bus = LaneBus::new();
        run_flow_cached(
            &mut flow,
            &mut cache,
            world,
            &mut bus,
            &runtime,
            context(false, Some(preview)),
        );
        world.end_run();
        sources(bus.get::<AudioView>().expect("the audio flow publishes"))
    };

    assert_eq!(run(&mut world), vec![marked], "the marked source");

    mark(&mut world, outside, preview);
    assert_eq!(
        run(&mut world),
        vec![marked, outside],
        "a source given the marker enters the scope"
    );

    unmark(&mut world, marked, preview);
    assert_eq!(
        run(&mut world),
        vec![outside],
        "a source whose marker is taken leaves the scope"
    );
}
