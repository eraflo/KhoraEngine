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

//! **An edit to the scene after a save reaches what the game did not
//! change**, through the whole road: the flow projects the scene, the agent
//! runs the lane, the writeback records what it observed, the game is saved
//! against its scene, the author edits a behavior's fields in the scene, and
//! the game loaded against the edited scene runs with the edit wherever the
//! game had left the field alone.

use std::sync::{Arc, Mutex};

use khora_agents::script_agent::ScriptAgent;
use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::Agent;
use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::event::Channel;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::Vec3;
use khora_core::scene::SerializationGoal;
use khora_core::script::{engine_event_channel, ScriptEvent, ScriptValue};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{Name, Script, ScriptState, TickPhase, Transform, World};
use khora_data::flow::{Flow, ScriptFlow};
use khora_io::serialization::SerializationService;
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;
use khora_script::vm::Program;

const MODULE: &str = "ai/guard.erg";

/// A guard with a field the game never changes and one it does.
const GUARD: &str = r#"
behavior Guard {
    int speed = 1;
    int health = 100;

    on Damaged(int amount) {
        health -= amount;
    }
}
"#;

fn compile(source: &str) -> Program {
    let lexed = khora_script::lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = khora_script::parse(lexed.tokens.clone());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = khora_script::check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = khora_script::compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// One running session, wired the way the engine wires one.
struct Session {
    world: World,
    flow: ScriptFlow,
    agent: ScriptAgent,
    scripts: Arc<Mutex<ScriptRuntime>>,
    events: Channel<ScriptEvent>,
    runtime: Arc<Runtime>,
}

impl Session {
    fn over(world: World) -> Self {
        let mut scripts = ScriptRuntime::new();
        scripts.add_program(MODULE, compile(GUARD));
        let scripts = Arc::new(Mutex::new(scripts));
        let events = engine_event_channel();

        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(events.clone());

        let mut agent = ScriptAgent::default();
        agent.apply_budget(ResourceBudget {
            strategy_id: StrategyId::Balanced,
            time_limit: std::time::Duration::from_millis(50),
            memory_limit: None,
            extra_params: Default::default(),
        });

        Self {
            world,
            flow: ScriptFlow::default(),
            agent,
            scripts,
            events,
            runtime: Arc::new(runtime),
        }
    }

    /// One frame: project, run the agent, then the frame-boundary systems
    /// that write back what it observed.
    fn frame(&mut self, delta: f32) {
        let selection = self.flow.select(&self.world, &self.runtime);
        let mut view = self.flow.project(&self.world, &selection, &self.runtime);
        view.delta_seconds = delta;

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

    /// A field of the guard, as the scripting world holds it.
    fn int_of(&self, guard: EntityId, field: &str) -> Option<i64> {
        let scripts = self.scripts.lock().expect("the scripting world");
        let slot = scripts
            .program(MODULE)
            .expect("loaded")
            .layout("Guard")
            .expect("declared")
            .slot_of(field)
            .expect("the field is declared");
        match scripts.peek(guard, "Guard")?.fields.get(slot)? {
            Persisted::Scalar(value) => value.as_int(),
            _ => None,
        }
    }
}

#[test]
fn a_scene_edit_after_a_save_reaches_the_field_the_game_left_alone() {
    let service = SerializationService::new();

    // The scene, as authored: speed 3, health 100.
    let mut world = World::new();
    let guard = world.spawn((
        Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        Name::new("Guard"),
        Script::new(MODULE, "Guard")
            .with_field("speed", ScriptValue::Int(3))
            .with_field("health", ScriptValue::Int(100)),
    ));
    let guard_id = world.mark_authored(guard).expect("authored");
    let base_file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let base_id = AssetUUID::new_v5("scenes/keep.kscene");

    // Play: the guard is hurt to 40; nothing touches its speed.
    let mut session = Session::over(world);
    session
        .events
        .send(ScriptEvent::new(guard, "Damaged").with(ScriptValue::Int(60)));
    session.frame(0.016);
    assert_eq!(
        (
            session.int_of(guard, "speed"),
            session.int_of(guard, "health")
        ),
        (Some(3), Some(40)),
        "the premise: hurt, at the authored speed"
    );
    let observed = session
        .world
        .get::<ScriptState>(guard)
        .expect("the writeback recorded what the lane observed");
    assert_eq!(
        observed.snapshot.field("speed"),
        Some(&ScriptValue::Int(3)),
        "the premise: the observed state holds every field, the untouched one included"
    );

    // Save the game, then quit.
    let save = service
        .save_game(
            &session.world,
            base_id,
            &base_file,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");
    drop(session);

    // The author edits the scene: speed 3 → 5, health 100 → 120.
    let mut editing = World::new();
    service
        .replace_world(&base_file, &mut editing)
        .expect("the scene opens");
    let edited_guard = editing
        .entity_with_id(guard_id)
        .expect("the guard is in the scene");
    let script = editing
        .get_mut::<Script>(edited_guard)
        .expect("the guard has its script");
    *script = Script::new(MODULE, "Guard")
        .with_field("speed", ScriptValue::Int(5))
        .with_field("health", ScriptValue::Int(120));
    let edited_base = service
        .save_world(&editing, SerializationGoal::EditorInterchange)
        .expect("the edited scene saves");

    // Load the game against the edited scene.
    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &edited_base)
        .expect("the game loads");
    let guard_back = loaded
        .entity_with_id(guard_id)
        .expect("the guard is back, under its identity");
    assert_eq!(
        loaded
            .get::<Script>(guard_back)
            .and_then(|s| s.field("speed")),
        Some(&ScriptValue::Int(5)),
        "the premise: the scene's edit reached the loaded `Script`"
    );

    let mut session = Session::over(loaded);
    session.frame(0.016);

    assert_eq!(
        (
            session.int_of(guard_back, "speed"),
            session.int_of(guard_back, "health")
        ),
        (Some(5), Some(40)),
        "speed, which the game left alone, takes the scene's edit; health keeps the game's 40"
    );
}
