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

//! **The watchdog in a session** (Ergon spec 10): a system whose `Update`
//! or timer never finishes is reported once past ten seconds of game time and
//! faults past sixty — its companion records the fault, an edit re-enables
//! it; frames the game does not play do not count; and a game save keeps
//! the count, so a body saved forty seconds in faults about twenty seconds
//! after loading.
//!
//! Game time is the `dt` the frames are given; where the plan leaves open
//! whether the frame a body starts in counts, the bounds allow either.

use khora_core::agent::EngineMode;
use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::scene::SerializationGoal;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;
use khora_lanes::script_lane::Slow;

use super::behavior_session::{attach, field, int, Project};
use super::system_session::{the_holder, SystemSession};

/// Fuel each frame: enough to enter a spinning body, never to finish it.
const SLICE: u64 = 50;

/// A system whose `Update` never finishes.
const SPIN: Project = &[(
    "game/spin.erg",
    "system Spin {
         int n = 0;
         void Update(float dt) { while (true) { n += 1; } }
     }",
)];

/// `Spin` with a field added — an edit to its module.
const SPIN_EDITED: Project = &[(
    "game/spin.erg",
    "system Spin {
         int n = 0;
         int laps = 0;
         void Update(float dt) { while (true) { n += 1; } }
     }",
)];

/// A system whose timer body never finishes.
const TICK: Project = &[(
    "game/tick.erg",
    "system Tick {
         int n = 0;
         every 1s { while (true) { n += 1; } }
     }",
)];

/// A behavior whose `Update` never finishes.
const SPINNER: Project = &[(
    "game/spinner.erg",
    "behavior Spinner {
         int n = 0;
         void Update(float dt) { while (true) { n += 1; } }
     }",
)];

/// A session watched frame by frame: every slow body its frames reported.
struct Watched {
    session: SystemSession,
    slow: Vec<Slow>,
    frames: usize,
    /// Whether the mode plays — the lane runs, and reports, only then.
    playing: bool,
}

impl Watched {
    fn new(world: World, project: Project) -> Self {
        let mut session = SystemSession::new(world, project);
        session.fuel = SLICE;
        Self {
            session,
            slow: Vec::new(),
            frames: 0,
            playing: true,
        }
    }

    /// Switches the engine's mode, as the editor's Play and Stop do.
    fn set_mode(&mut self, mode: EngineMode) {
        self.playing = mode.runs_gameplay();
        self.session.set_mode(mode);
    }

    /// `count` frames of one second.
    fn seconds(&mut self, count: usize) {
        for _ in 0..count {
            self.session.frame(1.0);
            self.frames += 1;
            // A frame the lane did not run reported nothing: the last report
            // is still the last playing frame's.
            if self.playing {
                self.slow
                    .extend(self.session.scripts.last_report().slow.iter().cloned());
            }
        }
    }

    /// Frames of one second until `name`'s companion on `entity` records a
    /// fault, at most `limit`; how many it took.
    fn seconds_until_faulted(
        &mut self,
        entity: EntityId,
        name: &str,
        limit: usize,
    ) -> Option<usize> {
        for second in 1..=limit {
            self.seconds(1);
            if faulted(&self.session.world, entity, name) {
                return Some(second);
            }
        }
        None
    }

    /// Frames of one second until a slow body of `name` is reported, at most
    /// `limit`.
    fn seconds_until_reported(&mut self, name: &str, limit: usize) {
        for _ in 0..limit {
            if self.warnings(name) > 0 {
                return;
            }
            self.seconds(1);
        }
    }

    fn warnings(&self, name: &str) -> usize {
        self.slow
            .iter()
            .filter(|slow| slow.behavior == name)
            .count()
    }
}

/// Whether `name`'s companion on `entity` records a fault.
fn faulted(world: &World, entity: EntityId, name: &str) -> bool {
    field(world, entity, &format!("{name}#state"), "fault")
        .is_some_and(|fault| fault != ScriptValue::Null)
}

/// **A system's spinning `Update` is reported once, then faults**: nothing
/// for nine seconds, one report by fourteen naming the system's entity and
/// the member, no fault through fifty-eight, its companion's fault by
/// sixty-four — and no more turns.
#[test]
fn a_systems_spinning_update_is_reported_once_then_faulted() {
    let mut watched = Watched::new(World::new(), SPIN);
    watched.seconds(9);
    assert_eq!(watched.warnings("Spin"), 0, "nine seconds is fine");
    watched.seconds_until_reported("Spin", 5);
    assert_eq!(watched.warnings("Spin"), 1, "reported past ten seconds");
    let spin = the_holder(&watched.session.world, "Spin");
    let slow = &watched.slow[0];
    assert_eq!((slow.entity, slow.member.as_str()), (spin, "Update"));
    assert!(!slow.site.is_empty(), "the site is named: {slow:?}");

    watched.seconds(58 - watched.frames);
    assert!(
        !faulted(&watched.session.world, spin, "Spin"),
        "58 s is no fault"
    );
    assert_eq!(watched.warnings("Spin"), 1, "reported once");
    let more = watched.seconds_until_faulted(spin, "Spin", 6);
    assert!(more.is_some(), "its companion records the fault by 64 s");

    let n = int(&watched.session.world, spin, "Spin", "n");
    watched.seconds(5);
    assert_eq!(
        int(&watched.session.world, spin, "Spin", "n"),
        n,
        "no more turns"
    );
}

/// **A system's spinning timer is reported, then faults.**
#[test]
fn a_systems_spinning_timer_is_reported_then_faulted() {
    let mut watched = Watched::new(World::new(), TICK);
    watched.seconds(9);
    assert_eq!(watched.warnings("Tick"), 0, "nine seconds is fine");
    watched.seconds_until_reported("Tick", 5);
    assert_eq!(watched.warnings("Tick"), 1, "reported past ten seconds");
    assert!(
        watched.slow[0].member.contains("every"),
        "the timer's member: {:?}",
        watched.slow[0]
    );
    let tick = the_holder(&watched.session.world, "Tick");
    watched.seconds(58 - watched.frames);
    assert!(!faulted(&watched.session.world, tick, "Tick"));
    assert!(watched.seconds_until_faulted(tick, "Tick", 6).is_some());
}

/// **An edit re-enables a system the watchdog faulted**: its companion's
/// fault is cleared and it runs again.
#[test]
fn an_edit_re_enables_a_system_the_watchdog_faulted() {
    let mut watched = Watched::new(World::new(), SPIN);
    watched.seconds(1);
    let spin = the_holder(&watched.session.world, "Spin");
    assert!(
        watched.seconds_until_faulted(spin, "Spin", 70).is_some(),
        "the watchdog faulted it"
    );
    let n = int(&watched.session.world, spin, "Spin", "n").expect("n");

    watched.session.recompile(SPIN_EDITED, &["game/spin.erg"]);
    watched.seconds(3);
    let world = &watched.session.world;
    assert!(!faulted(world, spin, "Spin"), "the edit cleared the fault");
    assert!(
        int(world, spin, "Spin", "n").expect("n") > n,
        "and it runs again"
    );
}

/// **Frames the game does not play do not count**: a spinning behavior run
/// eight seconds, then a hundred frames in the editor, is not reported, and
/// is within five seconds more of play; run to fifty-eight, then a hundred
/// frames in the editor, it has not faulted, and has within six more.
#[test]
fn frames_while_not_playing_do_not_count() {
    let mut watched = Watched::new(World::new(), SPINNER);
    let spinner = watched.session.world.spawn(Transform::identity());
    attach(&mut watched.session.world, spinner, "Spinner", &[]);
    let editor = || EngineMode::Custom("editor".to_owned());

    watched.seconds(8);
    watched.set_mode(editor());
    watched.seconds(100);
    watched.set_mode(EngineMode::Playing);
    watched.seconds(1);
    assert!(
        watched
            .session
            .scripts
            .peek(spinner, "Spinner")
            .is_some_and(|instance| instance.pending.is_some()),
        "the body is still part-way after the pause"
    );
    assert_eq!(watched.warnings("Spinner"), 0, "nine seconds of play");
    watched.seconds_until_reported("Spinner", 5);
    assert_eq!(
        watched.warnings("Spinner"),
        1,
        "reported within fourteen of play"
    );

    let played = watched.frames - 100;
    watched.seconds(58 - played);
    watched.set_mode(editor());
    watched.seconds(100);
    watched.set_mode(EngineMode::Playing);
    assert!(
        !faulted(&watched.session.world, spinner, "Spinner"),
        "58 s of play"
    );
    assert!(
        watched
            .seconds_until_faulted(spinner, "Spinner", 6)
            .is_some(),
        "faulted within sixty-four of play"
    );
}

/// **A game save keeps the count**: `Spin` saved forty seconds into its
/// `Update` faults about twenty seconds after the save loads, not sixty.
#[test]
fn a_game_save_of_a_spinning_system_keeps_its_count() {
    let service = SerializationService::new();
    let mut world = World::new();
    let authored = world.spawn(Transform::identity());
    world.mark_authored(authored).expect("authored");
    let mut watched = Watched::new(world, SPIN);
    let scene = service
        .save_world(&watched.session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    watched.seconds(40);
    let spin = the_holder(&watched.session.world, "Spin");
    assert!(!faulted(&watched.session.world, spin, "Spin"));
    assert_ne!(
        field(&watched.session.world, spin, "Spin#state", "pending"),
        Some(ScriptValue::Null),
        "the companion holds the body part-way"
    );
    let save = service
        .save_game(
            &watched.session.world,
            AssetUUID::new_v5("scenes/spin.kscene"),
            &scene,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");

    let mut loaded = World::new();
    service
        .load_game(&mut loaded, &save, &scene)
        .expect("the game loads");
    let mut resumed = Watched::new(loaded, SPIN);
    resumed.seconds(1);
    let spin = the_holder(&resumed.session.world, "Spin");
    let more = resumed.seconds_until_faulted(spin, "Spin", 70);
    let more = more.expect("the loaded system faults").saturating_add(1);
    assert!(
        (15..=25).contains(&more),
        "forty seconds were kept, so it faults about twenty later, not {more}"
    );
}
