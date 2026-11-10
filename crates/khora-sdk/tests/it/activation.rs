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

//! Which families run, from the application to the scheduler and the DCC:
//! the bootstrap publishes the runtime's `Activation` before the app's
//! `setup`, knowing every registered family and every one on; a family the
//! app switches off runs nothing — its scripts included — until it is
//! switched on; and the switch reaches the DCC.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::agent::UnknownFamily;
use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{
    Activation, AgentId, DeclaredComponentError, EngineApp, EngineCore, GameWorld, InputEvent,
    PhaseProvider, Runtime, WindowConfig,
};

const SCRIPT: AgentId = khora_sdk::khora_core::script::SCRIPT;
const PHYSICS: AgentId = khora_sdk::khora_core::physics::PHYSICS;
const AUDIO: AgentId = khora_sdk::khora_core::audio::AUDIO;

/// A game: every hook left to the engine's defaults.
struct Game;

impl PhaseProvider for Game {}
impl EngineApp for Game {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Game
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// The engine's `Activation`, as the bootstrap publishes it.
fn activation_of(runtime: &Runtime) -> Arc<Activation> {
    runtime
        .resources
        .get::<Arc<Activation>>()
        .cloned()
        .expect("the bootstrap publishes the engine's activation")
}

/// A tool built on the engine: its setup switches its scripts off, as an
/// editor's does, and, when its scripts are mounted, spawns one entity and
/// attaches `Greeter` — the behavior the mounted `greeter.erg` declares.
#[derive(Default)]
struct Tool {
    /// Whether setup attaches `Greeter`: only a tool whose scripts are
    /// mounted can.
    greeter: bool,
    /// What attaching `Greeter` in setup gave: the entity, or why not.
    attached: Option<Result<EntityId, DeclaredComponentError>>,
}

impl PhaseProvider for Tool {}
impl EngineApp for Tool {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Tool::default()
    }
    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        activation_of(runtime)
            .deactivate(SCRIPT)
            .expect("the scripting family is known at setup");
        if self.greeter {
            let entity = world.spawn(Transform::identity());
            self.attached = Some(
                world
                    .add_declared_component(entity, "Greeter", &[])
                    .map(|()| entity),
            );
        }
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

const MODULE: &str = "greeter.erg";

/// Counts its announcements in a field.
const GREETER: &str = "behavior Greeter {
                           int greeted = 0;
                           void OnSpawn() { greeted += 1; }
                       }";

/// A headless engine running `Tool`, its scripts mounted from `root`.
fn tool_engine(root: Option<&PathBuf>) -> EngineCore<Tool> {
    let mut runtime = Runtime::new();
    khora_sdk::scripts::install(&mut runtime);
    if let Some(root) = root {
        khora_sdk::scripts::mount(&runtime, root);
    }
    let mut engine = EngineCore::<Tool>::new();
    engine.bootstrap(
        Tool {
            greeter: root.is_some(),
            attached: None,
        },
        runtime,
    );
    engine
}

/// Ticks until `done` holds, or panics after a few seconds with `what`.
fn tick_until<A: EngineApp>(
    engine: &mut EngineCore<A>,
    what: &str,
    mut done: impl FnMut(&mut EngineCore<A>) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        engine.tick();
        if done(engine) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// **Every family is on after the bootstrap**: a game that switches nothing
/// runs everything.
#[test]
fn every_family_is_on_after_the_bootstrap() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    engine.tick();

    let activation = activation_of(engine.runtime());
    for family in [SCRIPT, PHYSICS, AUDIO, AgentId::new("renderer")] {
        assert!(activation.is_active(family), "{family}");
    }
    assert!(activation.inactive().is_empty());
}

/// **The engine's activation knows every registered family, and nothing
/// else.** Each family the engine registers — its lane families and the
/// script agent — can be switched; a name no agent negotiates as is refused.
#[test]
fn the_engines_activation_knows_every_registered_family() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    let activation = activation_of(engine.runtime());

    for family in [
        SCRIPT,
        PHYSICS,
        AUDIO,
        AgentId::new("renderer"),
        AgentId::new("shadows"),
        AgentId::new("ui"),
    ] {
        assert_eq!(activation.deactivate(family), Ok(()), "{family}");
    }
    let stranger = AgentId::new("no-such-family");
    assert_eq!(
        activation.deactivate(stranger),
        Err(UnknownFamily(stranger))
    );
}

/// **A tool's scripts switched off in setup run none until it switches them
/// on.** Setup attaches the behavior the mounted scripts declare. While the
/// scripting family is off no script runs, so its field keeps its default and
/// the engine wrote no state; once on, the entity arrives fresh and announces
/// itself, in its `Greeter` row.
#[test]
fn scripts_switched_off_in_setup_run_only_once_switched_on() {
    let root = assets_with_greeter();
    let mut engine = tool_engine(Some(&root));
    let entity = greeter(&mut engine);

    for _ in 0..30 {
        engine.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        greeted(&mut engine, entity),
        Some(ScriptValue::Int(0)),
        "scripts off: nothing ran"
    );
    assert!(
        !holds_greeter_state(&mut engine, entity),
        "scripts off: the engine wrote no state"
    );

    activation_of(engine.runtime())
        .activate(SCRIPT)
        .expect("a known family");
    tick_until(
        &mut engine,
        "OnSpawn to run once the scripts are on",
        |engine| greeted(engine, entity) == Some(ScriptValue::Int(1)),
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// **GORNA agrees with the frame.** The families off reach the DCC's context
/// once the boundary settles them, and so does switching them on again.
#[test]
fn a_switch_reaches_the_dcc() {
    let mut engine = tool_engine(None);
    let inactive = |engine: &mut EngineCore<Tool>| {
        engine
            .dcc()
            .map(|dcc| dcc.get_context().inactive)
            .expect("bootstrapped")
    };

    tick_until(
        &mut engine,
        "the DCC to learn the scripts are off",
        |engine| inactive(engine) == BTreeSet::from([SCRIPT]),
    );

    activation_of(engine.runtime())
        .activate(SCRIPT)
        .expect("a known family");
    tick_until(
        &mut engine,
        "the DCC to learn the scripts are on",
        |engine| inactive(engine).is_empty(),
    );
}

/// A fresh directory holding `assets/scripts/greeter.erg`.
fn assets_with_greeter() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "khora-sdk-activation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).expect("a scratch script directory");
    std::fs::write(scripts.join(MODULE), GREETER).expect("the script is written");
    root
}

/// The entity setup attached `Greeter` to.
fn greeter(engine: &mut EngineCore<Tool>) -> EntityId {
    match engine.app_mut().and_then(|tool| tool.attached.clone()) {
        Some(Ok(entity)) => entity,
        other => panic!("setup attaches the behavior its mounted scripts declare, got {other:?}"),
    }
}

/// The `greeted` field of `entity`'s `Greeter` row, if it holds one.
fn greeted(engine: &mut EngineCore<Tool>, entity: EntityId) -> Option<ScriptValue> {
    let world = engine.game_world_mut()?.inner_world();
    let key = world.components().key_named("Greeter")?;
    world
        .declared_row(entity, key)?
        .into_iter()
        .find_map(|(name, value)| (name == "greeted").then_some(value))
}

/// Whether `entity` holds the state the engine writes once `Greeter` ran.
fn holds_greeter_state(engine: &mut EngineCore<Tool>, entity: EntityId) -> bool {
    let Some(world) = engine.game_world_mut().map(|world| world.inner_world()) else {
        return false;
    };
    world
        .components()
        .key_named("Greeter#state")
        .is_some_and(|key| world.row(entity, key).is_some())
}

/// **Every linked `DataSystem` and `Flow` serves a family the engine
/// registers** — what the boot checks, and refuses otherwise.
#[test]
fn every_linked_role_serves_a_registered_family() {
    use khora_sdk::khora_data::ecs::{check_roles, registered_roles};

    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    let families: BTreeSet<AgentId> = engine
        .dcc()
        .expect("bootstrapped")
        .agent_registry()
        .lock()
        .expect("the registry lock")
        .all_ids()
        .into_iter()
        .collect();

    assert_eq!(check_roles(registered_roles(), &families), Ok(()));
}

/// **Whose work each engine registration is.** The family's work, or what it
/// produces, serves the family; a data invariant is upkeep.
#[test]
fn the_engines_registrations_declare_their_roles() {
    use khora_sdk::khora_data::ecs::{registered_roles, Role};

    let roles = registered_roles();
    let role_of = |name: &str| {
        roles
            .iter()
            .find(|(registered, _)| *registered == name)
            .map(|(_, role)| *role)
            .unwrap_or_else(|| panic!("`{name}` is registered"))
    };

    for (name, role) in [
        ("script_intake", Role::Serves(SCRIPT)),
        ("script", Role::Serves(SCRIPT)),
        ("script_systems", Role::Serves(SCRIPT)),
        ("script_commands", Role::Serves(SCRIPT)),
        ("behavior_writeback", Role::Serves(SCRIPT)),
        ("collision_to_script", Role::Serves(SCRIPT)),
        ("script_hot_reload", Role::Upkeep),
        ("script_components", Role::Upkeep),
        ("physics_debug_extraction", Role::Serves(PHYSICS)),
        ("physics_world_writeback", Role::Serves(PHYSICS)),
        ("physics_provider_sync", Role::Serves(PHYSICS)),
        ("collision_dispatch", Role::Serves(PHYSICS)),
        ("physics", Role::Serves(PHYSICS)),
        ("physics_release", Role::Upkeep),
        ("audio_playback_writeback", Role::Serves(AUDIO)),
        ("audio", Role::Serves(AUDIO)),
        ("transform_propagation", Role::Upkeep),
        ("ecs_maintenance", Role::Upkeep),
        ("render", Role::Upkeep),
    ] {
        assert_eq!(role_of(name), role, "{name}");
    }
}

/// **The scripting family's mailboxes, as the engine declares them.** The
/// engine's events are taken and dropped between sessions; a contact is read
/// by the `collision_to_script` cursor and dropped between sessions. A
/// recompiled module has no mailbox: it waits in the reserve, which holds it
/// across sessions. The `loading` family takes the pump's compile requests
/// and holds them between sessions.
#[test]
fn the_scripting_familys_mailboxes_are_declared() {
    use khora_sdk::khora_core::event::{BetweenSessions, Mailbox, Reader};

    let mailboxes = |message: &str| -> Vec<&Mailbox> {
        khora_sdk::inventory::iter::<Mailbox>
            .into_iter()
            .filter(|mailbox| (mailbox.message)().ends_with(message))
            .collect()
    };
    let declared = |message: &str| {
        let found = mailboxes(message);
        assert_eq!(found.len(), 1, "one mailbox of {message}: {found:?}");
        (found[0].family, found[0].reader, found[0].between_sessions)
    };

    assert_eq!(
        declared("::ScriptEvent"),
        (SCRIPT, Reader::Take, BetweenSessions::Drop)
    );
    assert!(
        mailboxes("::ScriptReload").is_empty(),
        "a recompiled module waits in the reserve, not a mailbox"
    );
    assert_eq!(
        declared("::CompileRequest"),
        (
            khora_sdk::AgentId::new("loading"),
            Reader::Take,
            BetweenSessions::Hold
        )
    );
    assert_eq!(
        declared("::Collision"),
        (
            SCRIPT,
            Reader::Cursor("collision_to_script"),
            BetweenSessions::Drop
        )
    );
}
