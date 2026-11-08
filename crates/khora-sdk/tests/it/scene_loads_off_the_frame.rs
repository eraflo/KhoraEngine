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

//! **A scene load is prepared off the frame and lands a few frames later,
//! end to end.** The engine boots the `loading` family and its hub; a
//! host files a load, the family's lane prepares it at the background, and
//! it is committed at a frame's boundary. A slow read never lengthens a
//! frame, and a game filed through the `GameWorld` lands as the synchronous
//! `load_game` gives it.
//!
//! A load spans frames, so these tests wait a bounded number of frames for
//! it, never assume the frame that files it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_sdk::khora_core::ecs::PersistentId;
use khora_sdk::khora_data::scene::loading::{LoadKind, SceneFetch, SceneSource};
use khora_sdk::khora_data::scene::NoPrefabs;
use khora_sdk::prelude::ecs::{Name, Transform};
use khora_sdk::prelude::math::Vec3;
use khora_sdk::prelude::AssetUUID;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, LoadOutcome,
    PhaseProvider, Runtime, SceneFile, SceneLoadHub, SerializationGoal, SerializationService,
    WindowConfig,
};

/// The most frames a load may take to land: the background run, the frame
/// start that hands it over, the boundary that commits it. Frames are ~10 ms
/// apart here, so this is seconds — a bound, not the frame that files.
const MAX_FRAMES: usize = 500;

/// How long the held read waits to be released before it reads anyway: far
/// longer than a frame, so a frame that waits on it shows.
const HOLD: Duration = Duration::from_secs(3);

/// A frame that waited on a held read lasts at least [`HOLD`]; one that did
/// not lasts a few milliseconds.
const LONG_FRAME: Duration = Duration::from_millis(1_500);

/// A game: every family on from the first frame.
struct Game;

impl AgentProvider for Game {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
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

fn game_engine() -> EngineCore<Game> {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());
    engine
}

/// One frame in the order a game's loop runs it.
fn frame(engine: &mut EngineCore<Game>) {
    let runtime = Arc::clone(engine.runtime());
    let inputs = engine.drain_inputs();
    engine.run_app_update(&inputs);
    let presents = engine.begin_render_frame(&runtime);
    engine.run_scheduler(&runtime);
    engine.end_render_frame(presents);
    engine.run_maintenance();
    engine.close_frame();
    std::thread::sleep(Duration::from_millis(10));
}

/// Frames until `landed` gives an outcome, at most [`MAX_FRAMES`]; returns
/// it, or panics naming `what`.
fn frames_until_landed(
    engine: &mut EngineCore<Game>,
    what: &str,
    mut landed: impl FnMut(&mut EngineCore<Game>) -> Option<LoadOutcome>,
) -> LoadOutcome {
    for _ in 0..MAX_FRAMES {
        frame(engine);
        if let Some(outcome) = landed(engine) {
            return outcome;
        }
    }
    panic!("{what} did not land within {MAX_FRAMES} frames");
}

/// The engine's scene load hub.
fn hub(engine: &EngineCore<Game>) -> Arc<SceneLoadHub> {
    engine
        .runtime()
        .resources
        .get::<Arc<SceneLoadHub>>()
        .cloned()
        .expect("the engine installs the scene load hub at boot")
}

/// A level as it ships — two authored entities — its scene file, and the
/// identity the game knows it by.
fn level() -> (GameWorld, SceneFile, AssetUUID) {
    let mut world = GameWorld::new();
    for (name, x) in [("Hero", 0.0), ("Door", 5.0)] {
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(x, 0.0, 0.0)),
            Name::new(name),
        ));
        world
            .inner_world_mut()
            .mark_authored(entity)
            .expect("a level's entities are authored");
    }
    let scene = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the level saves");
    (world, scene, AssetUUID::new_v5("levels/one.kscene"))
}

/// Every named entity of `world`: its identity, name and translation,
/// sorted by identity.
fn observed(world: &GameWorld) -> Vec<(Option<PersistentId>, String, Option<Vec3>)> {
    let mut seen: Vec<_> = world
        .iter_entities()
        .filter_map(|entity| {
            let name = world.get_component::<Name>(entity)?.as_str().to_owned();
            Some((
                world.inner_world().persistent_id(entity),
                name,
                world.get_transform(entity).map(|t| t.translation),
            ))
        })
        .collect();
    seen.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    seen
}

/// A scene's bytes, read only once the test lets them: it signals when the
/// read started, and waits to be released (at most [`HOLD`]).
struct HeldFetch {
    bytes: Vec<u8>,
    entered: AtomicBool,
    released: AtomicBool,
}

impl SceneFetch for HeldFetch {
    fn fetch(&self) -> Result<Vec<u8>, String> {
        self.entered.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + HOLD;
        while !self.released.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(self.bytes.clone())
    }
}

/// **A slow load never lengthens a frame**: while the scene's bytes are
/// held, the frames go on at their pace and the load stays in flight; once
/// released, it lands.
#[test]
fn a_slow_load_never_lengthens_a_frame() {
    let mut engine = game_engine();
    frame(&mut engine);
    let hub = hub(&engine);
    let (_, scene, _) = level();
    let fetch = Arc::new(HeldFetch {
        bytes: scene.to_bytes(),
        entered: AtomicBool::new(false),
        released: AtomicBool::new(false),
    });
    let ticket = hub.file(
        engine
            .game_world_mut()
            .expect("bootstrapped")
            .inner_world_mut(),
        LoadKind::Replace(SceneSource::Fetch(Arc::clone(&fetch) as Arc<dyn SceneFetch>)),
        Arc::new(NoPrefabs),
    );

    // Every frame from the one that files, until the read was held for
    // five more frames, is timed.
    let mut longest = Duration::ZERO;
    let mut after_entered = 0;
    for _ in 0..MAX_FRAMES {
        let started = Instant::now();
        frame(&mut engine);
        longest = longest.max(started.elapsed());
        if fetch.entered.load(Ordering::SeqCst) {
            after_entered += 1;
            if after_entered > 5 {
                break;
            }
        }
    }
    assert!(after_entered > 0, "the held read started");
    let pending_while_held = hub.is_pending(ticket);
    fetch.released.store(true, Ordering::SeqCst);

    assert!(
        longest < LONG_FRAME,
        "a frame lasted {longest:?} while the read was held"
    );
    assert!(
        pending_while_held,
        "the load waits for its read, the frames do not"
    );
    let outcome = frames_until_landed(&mut engine, "the load once released", |_| {
        hub.take_outcome(ticket)
    });
    assert!(outcome.is_ok(), "the load landed: {outcome:?}");
    let world = engine.game_world_mut().expect("bootstrapped");
    let names: Vec<String> = observed(world)
        .into_iter()
        .map(|(_, name, _)| name)
        .collect();
    assert_eq!(names.len(), 2, "the level's two entities: {names:?}");
}

/// **A game load filed through the SDK matches the synchronous one.** A
/// game saved over a level, then loaded twice: filed through the engine's
/// `GameWorld`, landing frames later, and loaded at once by `load_game`
/// into a fresh `GameWorld`. Both give the same entities, under the same
/// identities, with the same values, and the same report.
#[test]
fn a_game_load_filed_through_the_sdk_matches_the_synchronous_one() {
    let (mut played, scene, scene_id) = level();
    let hero = played
        .iter_entities()
        .find(|entity| {
            played
                .get_component::<Name>(*entity)
                .is_some_and(|n| n.as_str() == "Hero")
        })
        .expect("the hero");
    played.update_transform(hero, |t| t.translation = Vec3::new(2.0, 0.0, 1.0));
    played.spawn((
        Transform::from_translation(Vec3::new(9.0, 0.0, 0.0)),
        Name::new("Loot"),
    ));
    let save = played
        .save_game(scene_id, &scene, SerializationGoal::EditorInterchange)
        .expect("the game saves");

    let mut synchronous = GameWorld::new();
    let expected = synchronous
        .load_game(&save, &scene)
        .expect("the game loads at once");

    let mut engine = game_engine();
    frame(&mut engine);
    let ticket = engine
        .game_world_mut()
        .expect("bootstrapped")
        .file_load_game(&save, &scene);
    let outcome = frames_until_landed(&mut engine, "the filed game", |engine| {
        engine
            .game_world_mut()
            .and_then(|world| world.take_load_outcome(ticket))
    });

    let report = outcome.unwrap_or_else(|error| panic!("the filed game failed: {error}"));
    assert_eq!(report, expected, "the report the synchronous load gives");
    let filed = engine.game_world_mut().expect("bootstrapped");
    assert_eq!(observed(filed), observed(&synchronous));
    assert_eq!(observed(filed).len(), 3, "the hero, the door and the loot");
}

/// **Each game load filed on a world no engine runs has a ticket of its
/// own.** Three loads filed, the first's outcome taken in between: no
/// ticket names two loads, and each outcome is taken once, under its own.
#[test]
fn game_loads_filed_on_a_world_no_engine_runs_keep_their_own_tickets() {
    let (played, scene, scene_id) = level();
    let save = played
        .save_game(scene_id, &scene, SerializationGoal::EditorInterchange)
        .expect("the game saves");
    let mut world = GameWorld::new();

    let first = world.file_load_game(&save, &scene);
    let second = world.file_load_game(&save, &scene);
    assert!(
        world.take_load_outcome(first).is_some(),
        "the first landed at once"
    );
    let third = world.file_load_game(&save, &scene);

    assert_ne!(first, second);
    assert_ne!(second, third, "the third load took the second's ticket");
    assert_ne!(first, third, "the third load took the first's ticket");
    assert!(world.take_load_outcome(second).is_some(), "the second");
    assert!(
        world.take_load_outcome(third).is_some(),
        "the third has an outcome of its own"
    );
}
