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

//! The user's folders the engine keeps its files in, resolved once for the
//! hub, the editor, the SDK and the profiles: the home folder's `.khora`, a
//! game's data folder, and the quarantine list in each.
//!
//! Every folder here comes from a fake environment: no test reads the real
//! home or data folder.

use std::ffi::OsString;
use std::path::PathBuf;

use khora_core::app::GameIdentity;
use khora_io::profiles::ProfilePlace;
use khora_io::user_dirs::{data_dir, home_dir, khora_dir, quarantine_path};

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

/// Every variable a folder is read from, each set to a fake folder.
fn everything_set() -> impl Fn(&str) -> Option<OsString> {
    env_of(&[
        ("USERPROFILE", "fake-userprofile"),
        ("HOME", "fake-home"),
        ("APPDATA", "fake-appdata"),
        ("XDG_DATA_HOME", "fake-xdg"),
    ])
}

/// The fake home folder of [`everything_set`] on this platform.
fn fake_home() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("fake-userprofile")
    } else {
        PathBuf::from("fake-home")
    }
}

/// The fake data folder of [`everything_set`] on this platform.
fn fake_data() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("fake-appdata")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("fake-home")
            .join("Library")
            .join("Application Support")
    } else {
        PathBuf::from("fake-xdg")
    }
}

fn game(name: &str, version: &str) -> GameIdentity {
    GameIdentity {
        name: name.to_owned(),
        version: version.to_owned(),
    }
}

/// **The home and data folders follow the platform**, as the profiles read
/// them: `USERPROFILE` or `HOME`; `%APPDATA%`, the macOS application
/// support folder, or `$XDG_DATA_HOME`; none when the environment names
/// none, or names an empty one.
#[test]
fn the_user_folders_follow_the_platform() {
    let env = everything_set();
    assert_eq!(home_dir(&env), Some(fake_home()));
    assert_eq!(data_dir(&env), Some(fake_data()));
    assert_eq!(home_dir(&env_of(&[])), None);
    assert_eq!(data_dir(&env_of(&[])), None);
    assert_eq!(
        home_dir(&env_of(&[("USERPROFILE", ""), ("HOME", "")])),
        None,
        "an empty variable names no folder"
    );

    if cfg!(windows) {
        assert_eq!(home_dir(&env_of(&[("HOME", "fake-home")])), None);
    } else if !cfg!(target_os = "macos") {
        assert_eq!(
            data_dir(&env_of(&[("HOME", "fake-home")])),
            Some(PathBuf::from("fake-home").join(".local").join("share")),
            "no XDG_DATA_HOME: the home's .local/share"
        );
    }
}

/// **The engine's folder is the home's `.khora`**, and there is none
/// without a home.
#[test]
fn the_khora_folder_is_in_the_home() {
    assert_eq!(
        khora_dir(&everything_set()),
        Some(fake_home().join(".khora"))
    );
    assert_eq!(khora_dir(&env_of(&[])), None);
}

/// **The quarantine list is filed with the user's files**: a developer's in
/// `<home>/.khora/quarantine.ron`, whatever the game; a player's in
/// `<data>/<game>/khora/quarantine.ron`, the game's name made safe as one
/// folder name. None when the folder does not resolve.
#[test]
fn the_quarantine_list_is_filed_with_the_user_files() {
    let env = everything_set();

    assert_eq!(
        quarantine_path(ProfilePlace::Developer, &game("Ember", "0.3.1"), &env),
        Some(fake_home().join(".khora").join("quarantine.ron"))
    );
    assert_eq!(
        quarantine_path(ProfilePlace::Developer, &game("Other", "9"), &env),
        quarantine_path(ProfilePlace::Developer, &game("Ember", "0.3.1"), &env),
        "one list for every game a developer makes"
    );
    assert_eq!(
        quarantine_path(ProfilePlace::Player, &game("Ember", "0.3.1"), &env),
        Some(
            fake_data()
                .join("Ember")
                .join("khora")
                .join("quarantine.ron")
        )
    );
    assert_eq!(
        quarantine_path(ProfilePlace::Player, &game("../My Game!", "1"), &env),
        Some(
            fake_data()
                .join(khora_io::profiles::path_safe("../My Game!"))
                .join("khora")
                .join("quarantine.ron")
        ),
        "the game's name is one safe folder"
    );

    let empty = env_of(&[]);
    assert_eq!(
        quarantine_path(ProfilePlace::Developer, &game("Ember", "1"), &empty),
        None
    );
    assert_eq!(
        quarantine_path(ProfilePlace::Player, &game("Ember", "1"), &empty),
        None
    );
}
