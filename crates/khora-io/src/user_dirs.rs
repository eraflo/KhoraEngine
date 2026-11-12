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

//! The user's folders the engine keeps its files in: the home folder's
//! `.khora`, and a game's data folder.

use std::ffi::OsString;
use std::path::PathBuf;

use khora_core::app::GameIdentity;

use crate::profiles::{path_safe, ProfilePlace};

/// The user's home folder — `USERPROFILE` on Windows, `HOME` elsewhere — read
/// through `env`.
pub fn home_dir(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if cfg!(windows) {
        var(env, "USERPROFILE")
    } else {
        var(env, "HOME")
    }
}

/// `env`'s value for `key`, when it names something.
pub(crate) fn var(env: &dyn Fn(&str) -> Option<OsString>, key: &str) -> Option<PathBuf> {
    env(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The per-user data folder, read through `env`: `%APPDATA%` on Windows;
/// `$HOME/Library/Application Support` on macOS; `$XDG_DATA_HOME`, else
/// `$HOME/.local/share`, elsewhere.
pub fn data_dir(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if cfg!(windows) {
        var(env, "APPDATA")
    } else if cfg!(target_os = "macos") {
        home_dir(env).map(|home| home.join("Library").join("Application Support"))
    } else {
        var(env, "XDG_DATA_HOME")
            .or_else(|| home_dir(env).map(|home| home.join(".local").join("share")))
    }
}

/// `<home>/.khora`, read through `env`.
pub fn khora_dir(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    home_dir(env).map(|home| home.join(".khora"))
}

/// The quarantine list's file: `<home>/.khora/quarantine.ron` for a
/// developer, `<data>/<game>/khora/quarantine.ron` for a player.
pub fn quarantine_path(
    place: ProfilePlace,
    game: &GameIdentity,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let folder = match place {
        ProfilePlace::Developer => khora_dir(env)?,
        ProfilePlace::Player => data_dir(env)?.join(path_safe(&game.name)).join("khora"),
    };
    Some(folder.join(QUARANTINE_FILE))
}

/// The trust store's file, shared by the hub and the editor:
/// `<home>/.khora/trust.ron`, read through `env`.
pub fn trust_path(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    khora_dir(env).map(|khora| khora.join(TRUST_FILE))
}

/// The folder the build's signing key is kept in, never in a project:
/// `<home>/.khora/signing`, read through `env`.
pub fn signing_dir(env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    khora_dir(env).map(|khora| khora.join(SIGNING_DIR))
}

/// The folder the pack signing key is kept in, under the Khora folder.
const SIGNING_DIR: &str = "signing";

/// The trust store's file name.
const TRUST_FILE: &str = "trust.ron";

/// The quarantine list's file name.
const QUARANTINE_FILE: &str = "quarantine.ron";
