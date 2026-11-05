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

//! Scripts limited to a marker, on the road a frame takes: the flow projects
//! the entities holding the marker the substrate hands it, the lane runs
//! them, the writeback records them.
//!
//! An entity leaving the scope — its marker taken, or the scope narrowed —
//! is forgotten, not bid farewell: nothing despawned it, it only stopped
//! being run. An entity in the scope that is despawned is bid farewell, as
//! always.

use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_core::Runtime;
use khora_data::ecs::{ComponentKey, Transform, World};
use khora_data::flow::{Flow, FlowContext, ScriptFlow, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::native::Host;

use super::declared_behaviors::{attach, declared, key, write_back};

const MODULE: &str = "scoped.erg";

/// A marker, and a behavior that says goodbye, so a farewell is visible as an
/// event.
const KEEPER: &str = r#"component Preview {}
                       behavior Keeper {
                           int health = 100;
                           void OnDespawn() { this.Raise(Gone()); }
                       }
                       event Gone;"#;

/// A world declaring `KEEPER`, projected through the real flow and written
/// back through the engine's writeback.
struct Scene {
    world: World,
    flow: ScriptFlow,
    /// Whether the flow ran the frame before, as the substrate keeps it.
    ran: bool,
}

impl Scene {
    fn new() -> (Self, ScriptRuntime) {
        let (world, runtime) = declared(MODULE, KEEPER);
        let scene = Self {
            world,
            flow: ScriptFlow::default(),
            ran: false,
        };
        (scene, runtime)
    }

    /// The key of the `Preview` marker.
    fn preview(&self) -> ComponentKey {
        key(&self.world, "Preview")
    }

    /// Spawns an entity running `Keeper`, marked `Preview` when `marked`.
    fn keeper(&mut self, marked: bool) -> EntityId {
        let entity = self.world.spawn(Transform::identity());
        attach(&mut self.world, entity, "Keeper", &[]);
        if marked {
            attach(&mut self.world, entity, "Preview", &[]);
        }
        entity
    }

    /// This frame's view, as the flow projects it for a family scoped to
    /// `scope`.
    fn view(&mut self, scope: Option<ComponentKey>) -> ScriptView {
        let engine = Runtime::default();
        let ctx = FlowContext {
            resumed: !self.ran,
            scope,
        };
        self.ran = true;
        let selection = self.flow.select(&self.world, &engine, ctx);
        self.flow.project(&self.world, &selection, &engine)
    }

    /// One frame at full fuel, scoped to `scope`: the events it raised, as
    /// `(target, name)`.
    fn frame(
        &mut self,
        runtime: &mut ScriptRuntime,
        host: &mut Host,
        scope: Option<ComponentKey>,
    ) -> Vec<(EntityId, String)> {
        let view = self.view(scope);
        let report = run_behaviors(&view, &EventQueue::new(), runtime, host, u64::MAX);
        write_back(&mut self.world, &report);
        host.take_events()
            .as_slice()
            .iter()
            .map(|event| (event.target, event.name.clone()))
            .collect()
    }
}

fn farewells_of(raised: &[(EntityId, String)], entity: EntityId) -> usize {
    raised
        .iter()
        .filter(|(target, name)| *target == entity && name == "Gone")
        .count()
}

/// **An entity whose marker is taken is forgotten without a farewell**,
/// while a marked one despawned in the same frame is bid its farewell. Only
/// the marked keepers ever run.
#[test]
fn an_entity_leaving_the_scope_is_forgotten_without_a_farewell() {
    let (mut scene, mut runtime) = Scene::new();
    let preview = Some(scene.preview());
    let leaving = scene.keeper(true);
    let despawned = scene.keeper(true);
    let outside = scene.keeper(false);
    let mut host = Host::new();

    scene.frame(&mut runtime, &mut host, preview);
    scene.frame(&mut runtime, &mut host, preview);
    assert_eq!(
        runtime.instance_count(),
        2,
        "only the two marked keepers run"
    );
    assert!(runtime.peek(outside, "Keeper").is_none(), "never in scope");

    assert!(scene
        .world
        .remove_component_by_key(leaving, scene.preview()));
    scene.world.despawn(despawned);
    let mut raised = scene.frame(&mut runtime, &mut host, preview);
    raised.extend(scene.frame(&mut runtime, &mut host, preview));

    assert_eq!(
        farewells_of(&raised, despawned),
        1,
        "a despawned entity of the scope is bid farewell: {raised:?}"
    );
    assert_eq!(
        farewells_of(&raised, leaving),
        0,
        "leaving the scope is no despawn: {raised:?}"
    );
    assert!(
        runtime.peek(leaving, "Keeper").is_none(),
        "the entity that left the scope is forgotten"
    );
    assert_eq!(runtime.instance_count(), 0);
}

/// **A family narrowed to a scope drops what lies outside it without a
/// farewell.** Both keepers run while the scripts are on; scoped to
/// `Preview`, the unmarked one is forgotten, unbidden, and the marked one
/// carries on.
#[test]
fn entities_outside_a_new_scope_are_dropped_without_a_farewell() {
    let (mut scene, mut runtime) = Scene::new();
    let marked = scene.keeper(true);
    let outside = scene.keeper(false);
    let mut host = Host::new();

    scene.frame(&mut runtime, &mut host, None);
    scene.frame(&mut runtime, &mut host, None);
    assert_eq!(runtime.instance_count(), 2, "on: both run");

    let preview = Some(scene.preview());
    let mut raised = scene.frame(&mut runtime, &mut host, preview);
    raised.extend(scene.frame(&mut runtime, &mut host, preview));

    assert_eq!(
        farewells_of(&raised, outside),
        0,
        "narrowing the scope despawns nothing: {raised:?}"
    );
    assert!(
        runtime.peek(outside, "Keeper").is_none(),
        "the entity outside the scope is forgotten"
    );
    assert!(
        runtime.peek(marked, "Keeper").is_some(),
        "the marked one carries on"
    );
    assert_eq!(runtime.instance_count(), 1);
}

/// A behavior announcing its spawn and its load, so which way it came back is
/// visible as an event.
const ANNOUNCER: &str = r#"component Preview {}
                          behavior Keeper {
                              int health = 100;
                              void OnSpawn() { this.Raise(Born()); }
                              void OnLoad() { this.Raise(Loaded()); }
                          }
                          event Born;
                          event Loaded;"#;

fn raised_named(raised: &[(EntityId, String)], entity: EntityId, name: &str) -> usize {
    raised
        .iter()
        .filter(|(target, raised)| *target == entity && raised == name)
        .count()
}

/// **An entity coming back into the scope is loaded from nothing.** It ran
/// in the scope long enough for the writeback to record its state companion,
/// left, and came back: nothing was saved or loaded in between, so it is not
/// told `OnLoad`.
#[test]
fn an_entity_coming_back_into_the_scope_is_not_told_it_was_loaded() {
    let (world, runtime) = declared(MODULE, ANNOUNCER);
    let mut runtime = runtime;
    let mut scene = Scene {
        world,
        flow: ScriptFlow::default(),
        ran: false,
    };
    let preview = Some(scene.preview());
    let entity = scene.keeper(true);
    let mut host = Host::new();

    let mut raised = scene.frame(&mut runtime, &mut host, preview);
    raised.extend(scene.frame(&mut runtime, &mut host, preview));
    assert_eq!(raised_named(&raised, entity, "Born"), 1, "{raised:?}");
    assert!(
        super::declared_behaviors::recorded(&scene.world, entity, "Keeper").is_some(),
        "the writeback recorded its state companion"
    );

    assert!(scene.world.remove_component_by_key(entity, scene.preview()));
    scene.frame(&mut runtime, &mut host, preview);
    assert!(runtime.peek(entity, "Keeper").is_none(), "forgotten");

    attach(&mut scene.world, entity, "Preview", &[]);
    let mut back = scene.frame(&mut runtime, &mut host, preview);
    back.extend(scene.frame(&mut runtime, &mut host, preview));

    assert_eq!(
        raised_named(&back, entity, "Loaded"),
        0,
        "re-entering the scope loads nothing: {back:?}"
    );
}
