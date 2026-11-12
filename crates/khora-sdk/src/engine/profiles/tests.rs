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

//! A booted engine reads what the DCC learnt of this machine running this
//! game before its DCC starts, and writes it back when it shuts down: per
//! machine and game version for a developer, per machine for a player, whose
//! game may ship a profile to start from.
//!
//! Every test files its profiles under a scratch folder of its own, named by
//! `KHORA_PROFILES`, and journals the DCC's decisions there
//! (`KHORA_RECORD_DECISIONS`): the first round's corrections are what the
//! book was seeded with, before any frame ran. Each test sets these
//! variables for its own process: the tests run one process each.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_agents::registered_agent::registered_agents;
use khora_core::agent::gorna::protocol::{Epoch, Report};
use khora_core::agent::gorna::{AgentId, CostProfile, EpochRecord, ProfileRow, StrategyId};
use khora_core::lane::cost::PerDevice;
use khora_core::platform::MachineIdentity;
use khora_core::telemetry::TelemetryEvent;
use khora_io::profiles::{load, profile_path, save, ProfilePlace, PROFILES_VAR, SHIPPED_PROFILE};

use super::ShippedProfileDir;
use crate::engine::decision_journal::RECORD_VAR;
use crate::{
    EngineApp, EngineCore, GameIdentity, GameWorld, InputEvent, PhaseProvider, Runtime,
    WindowConfig, ENGINE_VERSION,
};

/// An app that names its game and does nothing else.
struct Profiled;

impl PhaseProvider for Profiled {}

impl EngineApp for Profiled {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn game_identity() -> GameIdentity {
        GameIdentity {
            name: "Profiled Game".to_owned(),
            version: "1.2.0".to_owned(),
        }
    }
    fn new() -> Self {
        Profiled
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A folder of its own under the system's temporary folder — profiles under
/// `profiles/`, the decisions' journal beside them — named to this process
/// by `KHORA_PROFILES` and `KHORA_RECORD_DECISIONS`; removed when dropped.
/// The process's home and data folders are made folders of it too, so an
/// engine keeping the user's files keeps them there, never in the home of
/// whoever runs the tests.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-sdk-profiles-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(root.join("profiles")).expect("a scratch folder");
        std::env::set_var(PROFILES_VAR, root.join("profiles"));
        std::env::set_var(RECORD_VAR, root.join("journal.json"));
        point_user_folders_at(&root);
        Scratch(root)
    }

    /// The folder every profile is filed under.
    fn profiles(&self) -> PathBuf {
        self.0.join("profiles")
    }

    /// The environment `profile_path` reads: `KHORA_PROFILES` alone.
    fn env(&self) -> impl Fn(&str) -> Option<OsString> {
        let profiles = self.profiles().into_os_string();
        move |name| (name == PROFILES_VAR).then(|| profiles.clone())
    }

    /// Where a profile of this machine running [`Profiled`] is filed, at
    /// `place`.
    fn path(&self, place: ProfilePlace) -> PathBuf {
        profile_path(
            place,
            &this_machine(),
            &Profiled::game_identity(),
            &self.env(),
        )
        .expect("KHORA_PROFILES names a folder")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The machine a headless engine runs on, as its boot reads it.
fn this_machine() -> MachineIdentity {
    khora_infra::platform::machine::read_machine(None)
}

/// A profile of `engine_version`, learnt on `machine` running `game`: every
/// family's three standard rows at `factor`, spread 0.2, over fifty runs. Any
/// row a family offers is planned at `factor` once seeded with it — the rows
/// it names at their own, the others at the mean of their agent's.
fn learnt_by_every_family(
    engine_version: &str,
    machine: MachineIdentity,
    game: GameIdentity,
    factor: f64,
) -> CostProfile {
    let rows = (registered_agents().iter())
        .flat_map(|registration| {
            [
                StrategyId::LowPower,
                StrategyId::Balanced,
                StrategyId::HighPerformance,
            ]
            .map(|row| ProfileRow {
                agent: registration.agent.name().to_owned(),
                row,
                factor,
                spread: 0.2,
                runs: 50,
            })
        })
        .collect();
    CostProfile {
        engine_version: engine_version.to_owned(),
        machine,
        game,
        rows,
    }
}

/// The engine, booted headless over `runtime`.
/// Names folders under `root` as the process's home (`USERPROFILE`, `HOME`)
/// and data folder (`APPDATA`, `XDG_DATA_HOME`).
fn point_user_folders_at(root: &std::path::Path) {
    std::env::set_var("USERPROFILE", root.join("home"));
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("APPDATA", root.join("data"));
    std::env::set_var("XDG_DATA_HOME", root.join("data"));
}

fn boot(runtime: Runtime) -> EngineCore<Profiled> {
    let mut engine = EngineCore::<Profiled>::new();
    engine.bootstrap(Profiled, runtime);
    let machine = engine
        .runtime()
        .resources
        .get::<Arc<MachineIdentity>>()
        .cloned()
        .expect("the boot names the machine");
    assert_eq!(*machine, this_machine(), "the machine the test files under");
    engine
}

/// The runtime a packed game boots over: a player's machine, its shipped
/// profile in `shipped`.
fn player_runtime(shipped: &Path) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(ProfilePlace::Player);
    runtime
        .resources
        .insert(ShippedProfileDir(shipped.to_path_buf()));
    runtime
}

/// Waits up to five seconds for `done`, polling every 10 ms.
fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

/// The first epoch `engine`'s DCC journaled: no frame has run, so its
/// corrections are what the cost book was seeded with.
fn first_epoch(engine: &EngineCore<Profiled>) -> EpochRecord {
    let dcc = engine.dcc().expect("the engine booted its DCC");
    let mut first = None;
    let opened = wait_for(|| {
        first = dcc.recorded_decisions().epochs.first().cloned();
        first.is_some()
    });
    assert!(opened, "the DCC opened its first epoch");
    first.expect("the first epoch")
}

/// Whether the first round planned every row at `factor`, and planned some.
fn planned_at(epoch: &EpochRecord, factor: f64) -> bool {
    !epoch.corrections.is_empty()
        && (epoch.corrections.iter())
            .all(|(_, _, correction)| (correction.factor - factor).abs() < 1e-6)
}

/// The first round's corrections, factor only, for a message.
fn factors(epoch: &EpochRecord) -> Vec<(AgentId, StrategyId, f64)> {
    (epoch.corrections.iter())
        .map(|(agent, row, correction)| (*agent, *row, correction.factor))
        .collect()
}

/// A run of `agent`'s `row` quoting 1 ms on the host and taking `factor`
/// times that, as the scheduler publishes it.
fn run_of(agent: AgentId, row: StrategyId, factor: f64) -> TelemetryEvent {
    TelemetryEvent::AgentReport(Report {
        agent,
        epoch: Epoch::default(),
        row,
        measured: PerDevice {
            cpu: Duration::from_secs_f64(factor / 1_000.0),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: Some(1_000.0),
        switch: None,
        message: None,
    })
}

/// **A profile from another engine version is not loaded.** A profile
/// filed for this machine and game, at the developer's path, learnt every
/// family's rows at 7:
/// - written by another engine version, it seeds nothing: the first round
///   plans every row at its quote;
/// - written by this version for another version of the game, it seeds
///   nothing either;
/// - written by this version for this game, it seeds the book: every row is
///   planned at 7.
#[test]
fn a_profile_from_another_engine_version_is_not_loaded() {
    let scratch = Scratch::new("version");
    let path = scratch.path(ProfilePlace::Developer);
    let game = Profiled::game_identity();
    let filed = |engine_version: &str, game: GameIdentity| {
        let profile = learnt_by_every_family(engine_version, this_machine(), game, 7.0);
        save(&path, &profile).expect("the profile is filed");
    };

    filed("0.0.0-another-engine", game.clone());
    let mut stale = boot(Runtime::new());
    let first = first_epoch(&stale);
    stale.shutdown();
    assert!(
        planned_at(&first, 1.0),
        "another engine's profile seeds nothing: {:?}",
        factors(&first)
    );

    filed(
        ENGINE_VERSION,
        GameIdentity {
            version: "9.9.9".to_owned(),
            ..game.clone()
        },
    );
    let mut other_game = boot(Runtime::new());
    let first = first_epoch(&other_game);
    other_game.shutdown();
    assert!(
        planned_at(&first, 1.0),
        "another game's profile seeds nothing: {:?}",
        factors(&first)
    );

    filed(ENGINE_VERSION, game);
    let mut current = boot(Runtime::new());
    let first = first_epoch(&current);
    current.shutdown();
    assert!(
        planned_at(&first, 7.0),
        "this engine's profile of this game seeds the book: {:?}",
        factors(&first)
    );
}

/// **A profile seeds the next run, and shutdown saves it.** With no profile
/// yet, the first round plans every row at its quote. The DCC learns a row
/// of the first family it planned at three times its quote; at shutdown, the
/// engine files what the DCC learnt at the developer's path — this engine's
/// version, this machine, this game, the rows the DCC learnt, no `.tmp` file
/// left — and the next boot plans that family's rows at the factor filed.
#[test]
fn a_profile_seeds_the_next_run_and_shutdown_saves_it() {
    let scratch = Scratch::new("saves");
    let path = scratch.path(ProfilePlace::Developer);
    assert!(!path.exists(), "no profile yet");

    let mut first_run = boot(Runtime::new());
    let first = first_epoch(&first_run);
    assert!(
        planned_at(&first, 1.0),
        "nothing learnt yet: {:?}",
        factors(&first)
    );
    let (agent, row, _) = first.corrections[0];
    let dcc = first_run.dcc().expect("the engine booted its DCC");
    let tx = dcc.event_sender();
    for _ in 0..30 {
        tx.send(run_of(agent, row, 3.0)).expect("the DCC listens");
    }
    // A family switched off — none of the engine's — opens an epoch, for a
    // DCC that publishes what it learnt once an epoch.
    let off = [AgentId::new("elsewhere")].into_iter().collect();
    tx.send(TelemetryEvent::ActivationChanged(off))
        .expect("the DCC listens");
    let learnt_row = |rows: &[ProfileRow]| {
        (rows.iter())
            .find(|learnt| learnt.agent == agent.name() && learnt.row == row)
            .map(|learnt| learnt.factor)
    };
    let learnt =
        wait_for(|| learnt_row(&dcc.learnt_costs()).is_some_and(|f| (f - 3.0).abs() < 0.01));
    assert!(
        learnt,
        "{agent}'s {row:?} learnt at 3: {:?}",
        dcc.learnt_costs()
    );

    first_run.shutdown();

    let rows = first_run.dcc().expect("still there").learnt_costs();
    let saved = load(&path)
        .expect("the profile reads")
        .expect("shutdown filed a profile at the developer's path");
    assert_eq!(saved.engine_version, ENGINE_VERSION);
    assert_eq!(saved.machine, this_machine());
    assert_eq!(saved.game, Profiled::game_identity());
    assert_eq!(saved.rows, rows, "the rows the DCC learnt");
    let factor = learnt_row(&saved.rows).expect("the row learnt is filed");
    assert!((factor - 3.0).abs() < 0.01, "{:?}", saved.rows);
    let folder = path.parent().expect("a profile has a folder");
    let leftovers: Vec<_> = std::fs::read_dir(folder)
        .expect("the folder lists")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .map(|entry| entry.file_name())
        .collect();
    assert!(leftovers.is_empty(), "no .tmp left: {leftovers:?}");

    let mut next_run = boot(Runtime::new());
    let next = first_epoch(&next_run);
    next_run.shutdown();
    let of_the_family: Vec<f64> = (next.corrections.iter())
        .filter(|(named, _, _)| *named == agent)
        .map(|(_, _, correction)| correction.factor)
        .collect();
    assert!(
        !of_the_family.is_empty() && of_the_family.iter().all(|f| (f - factor).abs() < 1e-6),
        "the next run plans {agent}'s rows at the factor filed, {factor}: {:?}",
        factors(&next)
    );
}

/// **A shipped profile is used as a prior, then refined.** A packed game
/// ships a profile learnt on the machine it was built on, every family's
/// rows at 7; this machine has no profile of its own yet.
/// - The first run is seeded from the shipped profile, whatever machine it
///   was learnt on: every row is planned at 7.
/// - At shutdown, this machine's own profile is filed at the player's path,
///   not the developer's.
/// - Once this machine has a profile of its own (every row at 3), it wins
///   over the shipped one.
#[test]
fn a_shipped_profile_is_used_as_prior_then_refined() {
    let scratch = Scratch::new("shipped");
    let shipped = scratch.0.join("game");
    std::fs::create_dir_all(&shipped).expect("the game's folder");
    let built_on = MachineIdentity {
        cpu: "the machine the game was built on".to_owned(),
        ..MachineIdentity::default()
    };
    save(
        &shipped.join(SHIPPED_PROFILE),
        &learnt_by_every_family(ENGINE_VERSION, built_on, Profiled::game_identity(), 7.0),
    )
    .expect("the game ships a profile");
    let local = scratch.path(ProfilePlace::Player);
    assert!(!local.exists(), "no profile of this machine yet");

    let mut first_run = boot(player_runtime(&shipped));
    let first = first_epoch(&first_run);
    first_run.shutdown();
    assert!(
        planned_at(&first, 7.0),
        "seeded from the shipped profile: {:?}",
        factors(&first)
    );
    assert!(
        local.is_file(),
        "shutdown files this machine's profile at the player's path"
    );
    assert!(
        !scratch.path(ProfilePlace::Developer).exists(),
        "a player's profile is not a developer's"
    );

    save(
        &local,
        &learnt_by_every_family(
            ENGINE_VERSION,
            this_machine(),
            Profiled::game_identity(),
            3.0,
        ),
    )
    .expect("this machine's profile is filed");
    let mut next_run = boot(player_runtime(&shipped));
    let next = first_epoch(&next_run);
    next_run.shutdown();
    assert!(
        planned_at(&next, 3.0),
        "this machine's own profile wins over the shipped one: {:?}",
        factors(&next)
    );
}

/// [`Profiled`], keeping its profiles as a game does.
struct Kept;

impl PhaseProvider for Kept {}

impl EngineApp for Kept {
    fn keeps_user_files() -> bool {
        true
    }
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn game_identity() -> GameIdentity {
        Profiled::game_identity()
    }
    fn new() -> Self {
        Kept
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// **A player with no data folder still starts from the shipped profile.**
/// A packed game ships a profile, every family's rows at 7; the player's
/// environment names no data folder (no `APPDATA`, `XDG_DATA_HOME`, `HOME`
/// nor `USERPROFILE`) and no `KHORA_PROFILES`, so this machine has no
/// profile of its own and nowhere to file one. The shipped profile is still
/// the prior: the first round plans every row at 7.
#[test]
fn a_player_with_no_data_folder_still_starts_from_the_shipped_profile() {
    let scratch = Scratch::new("no-data-folder");
    for name in [
        PROFILES_VAR,
        "APPDATA",
        "XDG_DATA_HOME",
        "HOME",
        "USERPROFILE",
    ] {
        std::env::remove_var(name);
    }
    let shipped = scratch.0.join("game");
    std::fs::create_dir_all(&shipped).expect("the game's folder");
    save(
        &shipped.join(SHIPPED_PROFILE),
        &learnt_by_every_family(
            ENGINE_VERSION,
            MachineIdentity::default(),
            Profiled::game_identity(),
            7.0,
        ),
    )
    .expect("the game ships a profile");

    let mut engine = EngineCore::<Kept>::new();
    engine.bootstrap(Kept, player_runtime(&shipped));
    let dcc = engine.dcc().expect("the engine booted its DCC");
    let mut first = None;
    let opened = wait_for(|| {
        first = dcc.recorded_decisions().epochs.first().cloned();
        first.is_some()
    });
    engine.shutdown();
    assert!(opened, "the DCC opened its first epoch");
    let first = first.expect("the first epoch");

    assert!(
        planned_at(&first, 7.0),
        "seeded from the shipped profile: {:?}",
        factors(&first)
    );
}

/// **A run that never shuts down keeps what it learnt.** The DCC learns a
/// row of the first family it planned at three times its quote; the engine
/// runs frames and is never shut down, as a run that crashes. Within
/// [`SAVE_EVERY`](super::SAVE_EVERY) of frames, and a little more, the
/// profile is filed at the developer's path, holding that row at 3.
#[test]
fn a_run_that_never_shuts_down_keeps_what_it_learnt() {
    let scratch = Scratch::new("never-shut-down");
    let path = scratch.path(ProfilePlace::Developer);

    let mut engine = boot(Runtime::new());
    let first = first_epoch(&engine);
    // A family that does not run here (no audio device): nothing it
    // measures overwrites the factor the test makes it learn.
    let (agent, row, _) = *(first.corrections.iter())
        .find(|(agent, _, _)| *agent == AgentId::new("audio"))
        .expect("the audio family was planned");
    let dcc = engine.dcc().expect("the engine booted its DCC");
    let tx = dcc.event_sender();
    for _ in 0..30 {
        tx.send(run_of(agent, row, 3.0)).expect("the DCC listens");
    }
    let off = [AgentId::new("elsewhere")].into_iter().collect();
    tx.send(TelemetryEvent::ActivationChanged(off))
        .expect("the DCC listens");
    let learnt_at_3 = |rows: &[ProfileRow]| {
        (rows.iter()).any(|learnt| {
            learnt.agent == agent.name() && learnt.row == row && (learnt.factor - 3.0).abs() < 0.01
        })
    };
    assert!(
        wait_for(|| learnt_at_3(&dcc.learnt_costs())),
        "{agent}'s {row:?} learnt at 3"
    );

    let deadline = Instant::now() + super::SAVE_EVERY + Duration::from_secs(5);
    let mut filed = None;
    while Instant::now() < deadline {
        engine.tick();
        filed = load(&path).ok().flatten().filter(|p| learnt_at_3(&p.rows));
        if filed.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    assert!(
        filed.is_some(),
        "the profile is filed while the engine runs, without a shutdown"
    );
}

/// **The engines these tests boot write no quarantine to the home folder.**
/// An app keeping the user's files ([`Kept`]) quarantines the files its run
/// refuses under the home folder, `.khora/quarantine.ron` for a developer.
/// Once a test's scratch folder is made and an engine booted, the home and
/// data folders the process names (`USERPROFILE`, `HOME`, `APPDATA`,
/// `XDG_DATA_HOME`) are scratch folders of the test's own, under the
/// system's temporary folder, not the ones it started with — checked before
/// anything is refused, so a failing run writes nothing there either. A
/// [`Kept`] engine then refuses a file: its quarantine lies under the scratch
/// home, and lists it.
#[test]
fn the_engines_these_tests_boot_write_no_quarantine_to_the_home_folder() {
    use khora_io::trust::{Layer, QuarantineKey, Refusals, SecurityReason, SecurityRefusal, Trust};
    use khora_io::user_dirs::{data_dir, home_dir, quarantine_path};

    let env = |key: &str| std::env::var_os(key);
    let real_home = home_dir(&env);
    let real_data = data_dir(&env);
    let temp = std::env::temp_dir();

    let scratch = Scratch::new("quarantine-home");
    let mut engine = boot(Runtime::new());
    engine.shutdown();

    let home = home_dir(&env).expect("the booted test names a home folder");
    assert_ne!(
        Some(&home),
        real_home.as_ref(),
        "the boot helper points the home folder away from the real one"
    );
    assert!(
        home.starts_with(&temp),
        "the home folder is a scratch folder: {home:?}"
    );
    let data = data_dir(&env).expect("the booted test names a data folder");
    assert_ne!(
        Some(&data),
        real_data.as_ref(),
        "the boot helper points the data folder away from the real one"
    );
    assert!(
        data.starts_with(&temp),
        "the data folder is a scratch folder: {data:?}"
    );

    let mut kept = EngineCore::<Kept>::new();
    kept.bootstrap(Kept, Runtime::new());
    let refusals = kept
        .runtime()
        .resources
        .get::<Arc<Refusals>>()
        .cloned()
        .expect("the bootstrap puts the refusals in the runtime");
    let refused = scratch.0.join("assets").join("lies.png");
    refusals.refuse(
        SecurityRefusal {
            path: refused.clone(),
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content([9; 32])),
    );
    kept.shutdown();

    let store = quarantine_path(ProfilePlace::Developer, &Kept::game_identity(), &env)
        .expect("a developer's quarantine under the home folder");
    assert!(store.starts_with(&home), "{store:?}");
    let listed: Vec<PathBuf> = Refusals::new(None, Some(store.clone()))
        .quarantined()
        .into_iter()
        .map(|refusal| refusal.path)
        .collect();
    assert_eq!(
        listed,
        vec![refused],
        "the refusal is quarantined under the scratch home: {store:?}"
    );
}
