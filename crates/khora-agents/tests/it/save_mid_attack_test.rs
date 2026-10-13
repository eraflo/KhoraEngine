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

//! **A guard saved mid-attack loads mid-attack**, through the whole road: the
//! flow projects the scene, the agent runs the lane, the writeback records
//! what the lane observed in the guard's state companion, the serialization
//! service saves the game against its scene, and a fresh world loaded from
//! the two finishes the attack.

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
use khora_data::ecs::{Name, TickPhase, Transform, World};
use khora_data::flow::{Flow, ScriptFlow};
use khora_io::script::declarations::{apply_declarations, DeclaredComponents};
use khora_io::serialization::SerializationService;
use khora_lanes::script_lane::ScriptRuntime;
use khora_script::arena::Persisted;

use super::behavior_session::{attach, compile, declared_in, field, int, Project};

const MODULE: &str = "ai/guard.erg";

/// A guard whose attack winds up for a second before it lands.
const GUARD: Project = &[(
    MODULE,
    r#"
behavior Guard {
    int health = 100;
    int fired = 0;

    async void Attack() {
        await 1.0s;
        fired += 1;
    }

    on Spotted(int by) {
        Attack();
    }
}
"#,
)];

/// Registers the guard's components — `Guard` and its state companion
/// `Guard#state` — in `world`, as the engine does once its scripts compile.
fn declare(world: &mut World) {
    let report = apply_declarations(
        world,
        &mut DeclaredComponents::default(),
        &[declared_in(MODULE, &compile(GUARD, MODULE))],
    );
    assert!(report.refused.is_empty(), "{report:?}");
}

/// One running session: its world, the flow projecting it, the agent and the
/// scripting world it drives, and the engine's event queue.
struct Session {
    world: World,
    flow: ScriptFlow,
    agent: ScriptAgent,
    scripts: Arc<Mutex<ScriptRuntime>>,
    events: Channel<ScriptEvent>,
    runtime: Arc<Runtime>,
}

impl Session {
    /// A session over `world`, wired the way the engine wires one, with the
    /// guard's module compiled and its components registered.
    fn over(mut world: World) -> Self {
        declare(&mut world);
        let mut scripts = ScriptRuntime::new();
        scripts.add_program(MODULE, compile(GUARD, MODULE));
        let scripts = Arc::new(Mutex::new(scripts));
        let events = engine_event_channel();

        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&scripts));
        runtime.resources.insert(events.clone());

        let mut agent = ScriptAgent::default();
        // The DCC does this every frame; without it there is no fuel and
        // every behavior is deferred.
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

    /// One frame of `delta` seconds: project, run the agent, then the
    /// frame-boundary systems that write back what it observed.
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

    /// The guard's `fired` counter, as the scripting world holds it.
    fn fired(&self, guard: EntityId) -> Option<i64> {
        let scripts = self.scripts.lock().expect("the scripting world");
        let slot = scripts
            .program(MODULE)
            .expect("loaded")
            .layout("Guard")
            .expect("declared")
            .slot_of("fired")
            .expect("`fired` is declared");
        match scripts.peek(guard, "Guard")?.fields.get(slot)? {
            Persisted::Scalar(value) => value.as_int(),
            _ => None,
        }
    }
}

/// The suspended sequence the guard's state companion records.
fn pending(world: &World, guard: EntityId) -> ScriptValue {
    field(world, guard, "Guard#state", "pending")
        .expect("the writeback recorded what the lane observed")
}

#[test]
fn a_guard_saved_mid_attack_loads_mid_attack() {
    let service = SerializationService::new();

    // The scene, as authored.
    let mut world = World::new();
    declare(&mut world);
    let guard = world.spawn((
        Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        Name::new("Guard"),
    ));
    attach(&mut world, guard, "Guard", &[]);
    let guard_id = world.mark_authored(guard).expect("authored");
    let base_file = service
        .save_world(&world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let base_id = AssetUUID::new_v5("scenes/keep.kscene");

    // Play: the guard spots someone and starts its attack.
    let mut session = Session::over(world);
    session
        .events
        .send(ScriptEvent::new(guard, "Spotted").with(ScriptValue::Int(1)));
    session.frame(0.016);
    assert_eq!(session.fired(guard), Some(0), "winding up");
    assert_ne!(
        pending(&session.world, guard),
        ScriptValue::Null,
        "the attack is caught part-way"
    );
    assert_eq!(
        (
            int(&session.world, guard, "Guard", "health"),
            int(&session.world, guard, "Guard", "fired")
        ),
        (Some(100), Some(0)),
        "the guard's component holds its fields, unchanged by the wind-up"
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

    // Load it in a fresh session.
    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &base_file)
        .expect("the game loads");
    let guard_back = loaded
        .entity_with_id(guard_id)
        .expect("the guard is back, under its identity");
    let mut session = Session::over(loaded);

    // Half a second is not the second the wind-up had left.
    session.frame(0.5);
    assert_eq!(session.fired(guard_back), Some(0), "still winding up");

    // The rest of it lands the attack — once.
    session.frame(0.6);
    assert_eq!(session.fired(guard_back), Some(1), "and then it fired");
    assert_eq!(
        int(&session.world, guard_back, "Guard", "fired"),
        Some(1),
        "written back to the guard's component"
    );
    assert_eq!(
        pending(&session.world, guard_back),
        ScriptValue::Null,
        "the sequence finished"
    );

    session.frame(1.5);
    assert_eq!(session.fired(guard_back), Some(1), "exactly once");
}
