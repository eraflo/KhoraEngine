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

//! A script reads what the player is holding.
//!
//! The whole road: a key arrives as an `InputEvent`, `input_map_update` folds it
//! into the action map, `ScriptFlow` snapshots the map into the view, the lane
//! puts the snapshot on the host, and `Pressed("jump")` answers.
//!
//! Until this existed the engine published an `InputMap` every frame that
//! nothing in the CLAD descent read.

use std::sync::{Arc, Mutex};

use khora_agents::script_agent::ScriptingAgent;
use khora_core::agent::Agent;
use khora_core::control::gorna::{ResourceBudget, StrategyId};
use khora_core::ecs::entity::EntityId;
use khora_core::event::Channel;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::platform::{input_channel, InputBinding, InputEvent, InputMap, KeyCode};
use khora_core::script::WorldCommand;
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{TickPhase, World};
use khora_data::flow::{ScriptFlow, ScriptInstance, ScriptProgram, ScriptView};
use khora_io::script_hot_reload::reload_channel;
use khora_script::reload::ScriptReload;

const MODULE: &str = "player.erg";

/// A player that despawns itself while the jump action is held.
///
/// Despawn because a `WorldCommand` on the deck is the only thing a test
/// outside the lane can observe, and it says unambiguously that the branch was
/// taken.
const PLAYER: &str = r#"
behavior Player {
    void Update(float dt) {
        if (Pressed("jump")) {
            Despawn(this);
        }
    }
}
"#;

fn compile(source: &str) -> khora_script::vm::Program {
    let parsed = khora_script::parse(khora_script::lex(source).tokens);
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

const PLAYER_ENTITY: EntityId = EntityId {
    index: 1,
    generation: 0,
};

fn a_scene_with_the_player(input: khora_core::platform::InputSnapshot) -> ScriptView {
    ScriptView {
        delta_seconds: 1.0 / 60.0,
        input,
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Player".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: PLAYER_ENTITY,
            program: 0,
            authored: None,
            translation: Default::default(),
            rotation: Default::default(),
            scale: Default::default(),
        }],
    }
}

/// One frame of the real descent: the input data system folds the events into
/// the map, the flow snapshots it, the agent runs the lane.
fn run_one_frame(
    agent: &mut ScriptingAgent,
    world: &mut World,
    runtime: &Arc<Runtime>,
) -> Vec<WorldCommand> {
    khora_control::substrate::run_data_systems(
        world,
        runtime,
        &mut OutputDeck::new(),
        TickPhase::PreSimulation,
    );

    let mut bus = LaneBus::new();
    let projected = ScriptFlow::default();
    let input = {
        use khora_data::flow::Flow;
        projected
            .project(world, &khora_data::flow::Selection::new(), runtime)
            .input
    };
    bus.publish(a_scene_with_the_player(input));

    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    {
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(runtime),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.execute(&mut ctx);
    }
    deck.take::<khora_core::script::CommandBuffer>()
        .as_slice()
        .to_vec()
}

fn wired() -> (Arc<Runtime>, Channel<InputEvent>, ScriptingAgent) {
    let mut map = InputMap::new();
    map.bind("jump", InputBinding::Key(KeyCode::Space));

    let events = input_channel();
    let reloads = reload_channel();
    reloads.send(ScriptReload {
        module: MODULE.to_owned(),
        program: compile(PLAYER),
    });

    let mut runtime = Runtime::default();
    runtime.resources.insert(events.clone());
    runtime.resources.insert(Arc::new(Mutex::new(map)));
    runtime.resources.insert(reloads);

    let mut agent = ScriptingAgent::default();
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::Balanced,
        time_limit: std::time::Duration::from_millis(1),
        memory_limit: None,
        extra_params: Default::default(),
    });

    (Arc::new(runtime), events, agent)
}

/// **`InputMap`'s first consumer inside the CLAD descent.** The engine had
/// published it every frame since it was written, and nothing read it.
#[test]
fn a_script_sees_the_action_the_player_is_holding() {
    let (runtime, events, mut agent) = wired();
    let mut world = World::new();

    // Frame one: nothing held. The program loads and the player does nothing.
    let quiet = run_one_frame(&mut agent, &mut world, &runtime);
    assert!(quiet.is_empty(), "nothing held; got {quiet:?}");

    events.send(InputEvent::KeyPressed {
        key_code: KeyCode::Space,
    });

    let queued = run_one_frame(&mut agent, &mut world, &runtime);

    assert!(
        queued.iter().any(|command| matches!(
            command,
            WorldCommand::Despawn { entity } if *entity == PLAYER_ENTITY
        )),
        "the script took the branch its action guards; got {queued:?}"
    );
}

/// Releasing is seen too — the snapshot is this frame's, not the frame the key
/// went down.
#[test]
fn a_released_action_stops_being_held() {
    let (runtime, events, mut agent) = wired();
    let mut world = World::new();
    run_one_frame(&mut agent, &mut world, &runtime);

    events.send(InputEvent::KeyPressed {
        key_code: KeyCode::Space,
    });
    assert!(!run_one_frame(&mut agent, &mut world, &runtime).is_empty());

    events.send(InputEvent::KeyReleased {
        key_code: KeyCode::Space,
    });

    assert!(
        run_one_frame(&mut agent, &mut world, &runtime).is_empty(),
        "the key came up, so the branch is not taken"
    );
}
