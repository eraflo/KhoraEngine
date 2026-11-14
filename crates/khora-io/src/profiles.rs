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

//! Where what the DCC learnt of a machine running a game is filed, and how a
//! profile is read and written.
//!
//! A profile is an untrusted file: it is read only under a size cap, and a
//! file that is not a profile is refused whole. Nothing here is ever sent
//! anywhere.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use khora_core::agent::gorna::CostProfile;
use khora_core::app::GameIdentity;
use khora_core::platform::MachineIdentity;

use crate::user_dirs::var;
pub use crate::user_dirs::{data_dir, home_dir};

/// Where a profile is filed: a player's machine, or a developer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilePlace {
    /// A developer's machine: the editor, the sandbox, a loose runtime.
    Developer,
    /// A player's machine: a packed game.
    Player,
}

/// The environment variable that, set, is the folder every profile is filed
/// under, in both places.
pub const PROFILES_VAR: &str = "KHORA_PROFILES";

/// The size past which a profile file is not read.
pub const MAX_PROFILE_BYTES: u64 = 1 << 20;

/// The profile a built game ships, beside its `runtime.json`.
pub const SHIPPED_PROFILE: &str = "profile.ron";

/// Returns `name` made safe as one path component.
///
/// ASCII letters, digits, `-`, `_` and `.` are kept, anything else is `_`; no
/// leading `.`, no Windows device name; never empty; at most 64 characters.
pub fn path_safe(name: &str) -> String {
    let mut safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    // No leading dot: never a hidden file, never `.` or `..`.
    let dots = safe.chars().take_while(|c| *c == '.').count();
    safe.replace_range(..dots, &"_".repeat(dots));
    safe.truncate(MAX_NAME);
    if safe.is_empty() {
        safe.push('_');
    }
    // A name Windows keeps for a device (`NUL`, `COM1`…), whatever its case
    // or extension, is no folder there: it is set apart.
    let stem = safe
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if device {
        safe.insert(0, '_');
        safe.truncate(MAX_NAME);
    }
    safe
}

/// The longest a [`path_safe`] name is.
const MAX_NAME: usize = 64;

/// Returns the file a profile of `machine` running `game` is filed in, `env` read for its folders.
///
/// - [`ProfilePlace::Developer`]:
///   `<root>/<machine key>/<game name>-<game version>.ron`, the root
///   `$KHORA_PROFILES`, else `<home>/.khora/profiles`;
/// - [`ProfilePlace::Player`]: `<root>/<machine key>.ron`, the root
///   `$KHORA_PROFILES/<game name>`, else `<data>/<game name>/khora/profiles`.
///
/// The game's name and version are made [`path_safe`]. `None` when no root
/// resolves.
pub fn profile_path(
    place: ProfilePlace,
    machine: &MachineIdentity,
    game: &GameIdentity,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let chosen = var(env, PROFILES_VAR);
    let name = path_safe(&game.name);
    match place {
        ProfilePlace::Developer => {
            let root = chosen
                .or_else(|| home_dir(env).map(|home| home.join(".khora").join("profiles")))?;
            Some(
                root.join(machine.key())
                    .join(format!("{name}-{}.ron", path_safe(&game.version))),
            )
        }
        ProfilePlace::Player => {
            let root = match chosen {
                Some(chosen) => chosen.join(&name),
                None => data_dir(env)?.join(&name).join("khora").join("profiles"),
            };
            Some(root.join(format!("{}.ron", machine.key())))
        }
    }
}

/// Returns the profile at `path`, or `Ok(None)` when there is no file.
///
/// # Errors
///
/// When the file is larger than [`MAX_PROFILE_BYTES`], cannot be read, or is
/// not a profile (RON).
pub fn load(path: &Path) -> std::io::Result<Option<CostProfile>> {
    let size = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if size > MAX_PROFILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("a profile of {size} bytes, over the {MAX_PROFILE_BYTES} read"),
        ));
    }
    let text = std::fs::read_to_string(path)?;
    ron::from_str(&text)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Writes `profile` at `path` (RON), atomically, making its folder.
///
/// # Errors
///
/// When the folder or the file cannot be written.
pub fn save(path: &Path, profile: &CostProfile) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let text = ron::ser::to_string_pretty(profile, ron::ser::PrettyConfig::default())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    // A `.tmp` of this write's own: two saves at once (two editors, two runs
    // of one game) never write into one file; the last rename wins whole.
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT_SAVE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp);
    if let Err(error) = std::fs::write(&tmp, text.as_bytes()) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Tells this process's saves apart.
static NEXT_SAVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
