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

//! Where what the DCC learnt of a machine running a game is filed — per
//! machine and game version, in a developer's home or a player's data folder
//! — and how a profile file is read and written: refused whole when it is
//! too large or not a profile, written atomically.
//!
//! Every folder here comes from a fake environment or a temporary folder:
//! no test reads the real home or data folder.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use khora_core::agent::gorna::{CostProfile, ProfileRow, StrategyId};
use khora_core::app::GameIdentity;
use khora_core::platform::MachineIdentity;
use khora_io::profiles::{
    data_dir, home_dir, load, path_safe, profile_path, save, ProfilePlace, MAX_PROFILE_BYTES,
    PROFILES_VAR, SHIPPED_PROFILE,
};

/// An environment holding `vars` and nothing else.
fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: Vec<(String, OsString)> = (vars.iter())
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect();
    move |name| {
        (vars.iter())
            .find(|(named, _)| named == name)
            .map(|(_, value)| value.clone())
    }
}

/// A machine named by its CPU alone.
fn machine(cpu: &str) -> MachineIdentity {
    MachineIdentity {
        gpu: None,
        cpu: cpu.to_owned(),
        cores: 8,
        os: "Some OS 11".to_owned(),
    }
}

fn game(name: &str, version: &str) -> GameIdentity {
    GameIdentity {
        name: name.to_owned(),
        version: version.to_owned(),
    }
}

/// A profile of `game` on `machine`, its renderer's floor learnt at
/// `factor`.
fn profile_of(machine: MachineIdentity, game: GameIdentity, factor: f64) -> CostProfile {
    CostProfile {
        engine_version: "0.9.1".to_owned(),
        machine,
        game,
        rows: vec![
            ProfileRow {
                agent: "renderer".to_owned(),
                row: StrategyId::LowPower,
                factor,
                spread: 0.125,
                runs: 240,
            },
            ProfileRow {
                agent: "physics".to_owned(),
                row: StrategyId::Custom(3),
                factor: 0.5,
                spread: 0.0,
                runs: 7,
            },
        ],
    }
}

/// Every environment variable a folder is read from, each set to a fake
/// folder named after it.
fn everything_set() -> impl Fn(&str) -> Option<OsString> {
    env_of(&[
        ("USERPROFILE", "fake-userprofile"),
        ("HOME", "fake-home"),
        ("APPDATA", "fake-appdata"),
        ("XDG_DATA_HOME", "fake-xdg-data"),
    ])
}

/// The home folder [`everything_set`] names on this platform.
fn fake_home() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("fake-userprofile")
    } else {
        PathBuf::from("fake-home")
    }
}

/// The data folder [`everything_set`] names on this platform.
fn fake_data() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("fake-appdata")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("fake-home")
            .join("Library")
            .join("Application Support")
    } else {
        PathBuf::from("fake-xdg-data")
    }
}

/// Whether `name` is safe as one path component: not empty, at most 64
/// characters, only ASCII letters, digits, `-`, `_` and `.`, not starting
/// with `.`.
fn is_one_safe_component(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 64
        && !name.starts_with('.')
        && (name.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// **A profile is stored per machine and game version.**
/// - A developer's: `<home>/.khora/profiles/<machine key>/<game>-<version>.ron`;
///   a player's: `<data>/<game>/khora/profiles/<machine key>.ron`.
/// - `KHORA_PROFILES`, set, is the folder both are filed under:
///   `<it>/<machine key>/<game>-<version>.ron` and
///   `<it>/<game>/<machine key>.ron`.
/// - Another version of the game is another file for a developer; another
///   machine is another file in both places.
/// - No folder to file under, no path.
/// - A game's name and version are made safe as one path component each:
///   `../x` or `a/b:c` never leave the folder, an empty name is `_`, a long
///   one is cut to 64 characters.
#[test]
fn a_profile_is_stored_per_machine_and_game_version() {
    assert_eq!(PROFILES_VAR, "KHORA_PROFILES");
    assert_eq!(SHIPPED_PROFILE, "profile.ron");
    let here = machine("Cpu Model X");
    let key = here.key();
    let keep = game("Lantern Keep", "1.4.0");
    let env = everything_set();

    assert_eq!(
        profile_path(ProfilePlace::Developer, &here, &keep, &env),
        Some(
            fake_home()
                .join(".khora")
                .join("profiles")
                .join(&key)
                .join("Lantern_Keep-1.4.0.ron")
        ),
        "a developer's, under the home folder"
    );
    assert_eq!(
        profile_path(ProfilePlace::Player, &here, &keep, &env),
        Some(
            fake_data()
                .join("Lantern_Keep")
                .join("khora")
                .join("profiles")
                .join(format!("{key}.ron"))
        ),
        "a player's, under the data folder"
    );

    let filed = env_of(&[
        (PROFILES_VAR, "filed-here"),
        ("USERPROFILE", "fake-userprofile"),
        ("HOME", "fake-home"),
        ("APPDATA", "fake-appdata"),
        ("XDG_DATA_HOME", "fake-xdg-data"),
    ]);
    assert_eq!(
        profile_path(ProfilePlace::Developer, &here, &keep, &filed),
        Some(
            PathBuf::from("filed-here")
                .join(&key)
                .join("Lantern_Keep-1.4.0.ron")
        ),
        "KHORA_PROFILES wins over the home folder"
    );
    assert_eq!(
        profile_path(ProfilePlace::Player, &here, &keep, &filed),
        Some(
            PathBuf::from("filed-here")
                .join("Lantern_Keep")
                .join(format!("{key}.ron"))
        ),
        "KHORA_PROFILES wins over the data folder"
    );

    let next = game("Lantern Keep", "1.5.0");
    let there = machine("Cpu Model Y");
    assert_ne!(
        profile_path(ProfilePlace::Developer, &here, &keep, &env),
        profile_path(ProfilePlace::Developer, &here, &next, &env),
        "a developer's: another version, another file"
    );
    for place in [ProfilePlace::Developer, ProfilePlace::Player] {
        assert_ne!(
            profile_path(place, &here, &keep, &env),
            profile_path(place, &there, &keep, &env),
            "{place:?}: another machine, another file"
        );
        assert_eq!(
            profile_path(place, &here, &keep, &env_of(&[])),
            None,
            "{place:?}: nowhere to file it"
        );
    }

    assert_eq!(path_safe("Lantern Keep"), "Lantern_Keep");
    assert_eq!(path_safe("1.4.0-beta_2"), "1.4.0-beta_2");
    assert_eq!(path_safe("a/b:c"), "a_b_c");
    assert_eq!(path_safe("a\\b"), "a_b");
    assert_eq!(path_safe("café"), "caf_");
    assert_eq!(path_safe(""), "_");
    assert_eq!(path_safe(&"a".repeat(100)), "a".repeat(64));
    for name in ["../x", "..", ".", ".hidden", "/", "C:", "a/../../b", ""] {
        let safe = path_safe(name);
        assert!(is_one_safe_component(&safe), "{name:?} made {safe:?}");
    }

    let escaping = game("../../escape", "../1");
    let root = PathBuf::from("filed-here");
    let developer = profile_path(ProfilePlace::Developer, &here, &escaping, &filed)
        .expect("a developer's path");
    assert_eq!(
        developer.parent(),
        Some(root.join(&key).as_path()),
        "the game's name stays in the machine's folder: {developer:?}"
    );
    let file = (developer.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    assert!(
        is_one_safe_component(file.trim_end_matches(".ron")),
        "{developer:?}"
    );
    let player =
        profile_path(ProfilePlace::Player, &here, &escaping, &filed).expect("a player's path");
    assert_eq!(
        player.parent().and_then(Path::parent),
        Some(root.as_path()),
        "the game's folder stays in the profiles' folder: {player:?}"
    );
    let folder = (player.parent())
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    assert!(is_one_safe_component(folder), "{player:?}");
}

/// **The home and data folders follow the platform**, read through the
/// environment given:
/// - the home folder is `USERPROFILE` on Windows, `HOME` elsewhere;
/// - the data folder is `%APPDATA%` on Windows,
///   `$HOME/Library/Application Support` on macOS, and `$XDG_DATA_HOME`, else
///   `$HOME/.local/share`, elsewhere;
/// - an environment naming none has neither.
#[test]
fn the_data_dir_follows_the_platform() {
    let env = everything_set();
    assert_eq!(home_dir(&env), Some(fake_home()));
    assert_eq!(data_dir(&env), Some(fake_data()));
    assert_eq!(home_dir(&env_of(&[])), None);
    assert_eq!(data_dir(&env_of(&[])), None);

    if cfg!(windows) {
        assert_eq!(
            home_dir(&env_of(&[("HOME", "fake-home")])),
            None,
            "Windows reads USERPROFILE"
        );
        assert_eq!(
            data_dir(&env_of(&[("HOME", "fake-home"), ("USERPROFILE", "u")])),
            None,
            "Windows reads APPDATA"
        );
    } else if cfg!(target_os = "macos") {
        assert_eq!(
            data_dir(&env_of(&[("HOME", "fake-home")])),
            Some(
                PathBuf::from("fake-home")
                    .join("Library")
                    .join("Application Support")
            )
        );
    } else {
        assert_eq!(
            data_dir(&env_of(&[("HOME", "fake-home")])),
            Some(PathBuf::from("fake-home").join(".local").join("share")),
            "no XDG_DATA_HOME: the home's .local/share"
        );
        assert_eq!(
            home_dir(&env_of(&[("USERPROFILE", "u")])),
            None,
            "HOME is read here"
        );
    }
}

/// `profile` written as RON, then padded with spaces to `size` bytes: a
/// profile still, whatever its size.
fn padded(profile: &CostProfile, size: u64) -> Vec<u8> {
    let mut bytes = ron::ser::to_string(profile)
        .expect("a profile writes as RON")
        .into_bytes();
    let size = usize::try_from(size).expect("a size in memory");
    assert!(bytes.len() <= size, "the profile fits {size} bytes");
    bytes.resize(size, b' ');
    bytes
}

/// **A corrupt or oversized profile is ignored.** Read back:
/// - no file is no profile, and no error;
/// - a file that is not RON, or RON that is not a profile, is refused;
/// - a folder where the file should be is refused;
/// - a profile of exactly 1 MiB is read; one byte more is refused, though
///   it is a profile: the file is not read past the cap.
#[test]
fn a_corrupt_or_oversized_profile_is_ignored() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let at = |name: &str| dir.path().join(name);
    let written = profile_of(machine("Cpu Model X"), game("Lantern Keep", "1.4.0"), 1.5);

    let absent = load(&at("absent.ron"));
    assert!(matches!(absent, Ok(None)), "no file: {absent:?}");

    std::fs::write(at("garbage.ron"), b"\x00\xffnot a profile {{{").expect("written");
    assert!(load(&at("garbage.ron")).is_err(), "not RON");
    std::fs::write(at("other.ron"), b"(foo: 1, bar: \"baz\")").expect("written");
    assert!(load(&at("other.ron")).is_err(), "RON, not a profile");
    std::fs::create_dir(at("folder.ron")).expect("a folder");
    assert!(load(&at("folder.ron")).is_err(), "a folder, not a file");

    assert_eq!(MAX_PROFILE_BYTES, 1 << 20);
    std::fs::write(at("full.ron"), padded(&written, MAX_PROFILE_BYTES)).expect("written");
    let full = load(&at("full.ron"));
    assert!(
        matches!(&full, Ok(Some(read)) if *read == written),
        "a profile of exactly the cap is read: {:?}",
        full.as_ref()
            .map(|read| read.as_ref().map(|p| p.rows.len()))
    );
    std::fs::write(at("over.ron"), padded(&written, MAX_PROFILE_BYTES + 1)).expect("written");
    assert!(
        load(&at("over.ron")).is_err(),
        "one byte over the cap is refused, though it is a profile"
    );
}

/// The names of the entries of `folder`.
fn entries(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder)
        .expect("the folder lists")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// **A profile is saved atomically.** Saved where no folder exists yet, the
/// folders are made; the file is RON, and reads back as the profile saved;
/// no `.tmp` file is left beside it. Saved again over it, the file is the
/// second profile, and still alone in its folder.
#[test]
fn a_profile_is_saved_atomically() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let folder = dir.path().join("a").join("b").join("c");
    let path = folder.join("profile.ron");
    let first = profile_of(machine("Cpu Model X"), game("Lantern Keep", "1.4.0"), 1.5);
    let second = profile_of(machine("Cpu Model X"), game("Lantern Keep", "1.4.0"), 0.25);

    save(&path, &first).expect("the profile saves");

    assert!(path.is_file(), "the file and its folders are made");
    let text = std::fs::read_to_string(&path).expect("the file reads");
    let parsed: CostProfile = ron::from_str(&text).expect("the file is a profile in RON");
    assert_eq!(parsed, first);
    assert_eq!(
        load(&path).expect("the profile reads back"),
        Some(first.clone())
    );
    assert_eq!(
        entries(&folder),
        vec!["profile.ron".to_owned()],
        "no .tmp left"
    );

    save(&path, &second).expect("the profile saves over the first");

    assert_eq!(load(&path).expect("the profile reads back"), Some(second));
    assert_eq!(
        entries(&folder),
        vec!["profile.ron".to_owned()],
        "no .tmp left"
    );
}

/// **Two saves of one profile at once leave a profile.** Two runs of one game
/// on one machine — two editors on one project, a game and its editor — file
/// their profile at the same path, each when it likes. One run's save has
/// made its `.tmp` and is about to write it when the other's saves whole;
/// the first then writes its own, shorter, and renames it. Whichever lands,
/// the file reads back as one of the two profiles: never one written over
/// the other's tail.
#[test]
fn two_saves_of_one_profile_at_once_leave_a_profile() {
    use std::io::Write;
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let path = dir.path().join("profile.ron");
    let long = {
        let mut long = profile_of(machine("Cpu Model X"), game("Lantern Keep", "1.4.0"), 1.5);
        let row = long.rows[0].clone();
        long.rows.extend(std::iter::repeat_n(row, 20));
        long
    };
    let short = profile_of(machine("Cpu Model X"), game("Lantern Keep", "1.4.0"), 0.25);
    let short_text = ron::ser::to_string_pretty(&short, ron::ser::PrettyConfig::default())
        .expect("a profile writes as RON");
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);

    // The first save has made its `.tmp`, as `save` does, and not written it.
    let mut first = std::fs::File::create(&tmp).expect("the first save's .tmp");
    // The second saves whole.
    save(&path, &long).expect("the second save");
    // The first writes its profile and renames it.
    first
        .write_all(short_text.as_bytes())
        .expect("the first save writes");
    drop(first);
    let _ = std::fs::rename(&tmp, &path);

    let read = load(&path);
    assert!(
        matches!(&read, Ok(Some(profile)) if *profile == long || *profile == short),
        "the file is one of the two profiles: {:?}",
        read.as_ref()
            .map(|read| read.as_ref().map(|p| p.rows.len()))
    );
}

/// **A game named like a device still files its profile.** On Windows,
/// `NUL`, `CON`, `AUX`, `PRN`, `COM1`… are devices, whatever their case: a
/// folder so named cannot hold a file. A player's profile of a game so
/// named — "Nul" is a word — is still saved in its game's folder and reads
/// back.
#[test]
fn a_game_named_like_a_device_still_files_its_profile() {
    let dir = tempfile::TempDir::new().expect("a temp dir");
    let root = dir.path().to_str().expect("a UTF-8 temp dir").to_owned();
    let env = env_of(&[(PROFILES_VAR, root.as_str())]);
    let here = machine("Cpu Model X");
    for name in ["Nul", "NUL", "CON", "AUX", "PRN", "COM1", "LPT1"] {
        let named = game(name, "1.0.0");
        let path =
            profile_path(ProfilePlace::Player, &here, &named, &env).expect("a player's path");
        let profile = profile_of(here.clone(), named, 1.5);

        let saved = save(&path, &profile);

        assert!(saved.is_ok(), "{name:?} at {path:?}: {saved:?}");
        assert_eq!(
            load(&path).expect("the profile reads back"),
            Some(profile),
            "{name:?}"
        );
    }
}
