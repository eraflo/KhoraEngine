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

//! What the DCC learnt of this machine running this game, carried from one
//! run to the next: read before the DCC starts and handed to its cost book,
//! written when the engine shuts down.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use khora_control::DccService;
use khora_core::agent::gorna::{CostProfile, ProfileRow};
use khora_core::app::GameIdentity;
use khora_core::platform::MachineIdentity;
use khora_core::Runtime;
use khora_io::profiles::{load, profile_path, save, ProfilePlace, PROFILES_VAR, SHIPPED_PROFILE};

/// The folder a packed game ships the profile it was built with in — the
/// folder its `runtime.json` sits in. The default runtime puts it in the
/// runtime's resources beside [`ProfilePlace::Player`]; without it, a
/// player's boot reads no shipped profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShippedProfileDir(pub(crate) PathBuf);

/// How often, at most, what the DCC learnt is written while the game runs:
/// a run that ends without shutting down keeps what it learnt until then.
const SAVE_EVERY: Duration = Duration::from_secs(5);

/// The profile of this machine running this game: where it is filed, and
/// what was last written there.
pub(crate) struct Profiles {
    path: PathBuf,
    machine: MachineIdentity,
    game: GameIdentity,
    saved: Vec<ProfileRow>,
    saved_at: Instant,
}

impl Profiles {
    /// Reads the profile of the machine and game `runtime` names, before the
    /// DCC starts, and seeds `dcc`'s cost book with it. Kept only when the
    /// application keeps profiles (`kept`) or `KHORA_PROFILES` names a
    /// folder: an application that asked for none — a test's — never reads
    /// nor writes the user's folders. `None` when profiles are not kept, or
    /// no folder resolves.
    ///
    /// The local profile is the one filed under this machine; a player's
    /// boot with none reads the one its game shipped. A profile is used only
    /// when it was learnt by this engine version running this game; any
    /// other, or one that does not read, is logged and left.
    pub(crate) fn begin(runtime: &Runtime, dcc: &DccService, kept: bool) -> Option<Self> {
        let chosen = std::env::var_os(PROFILES_VAR).is_some_and(|folder| !folder.is_empty());
        if !kept && !chosen {
            return None;
        }
        let machine = runtime
            .resources
            .get::<Arc<MachineIdentity>>()?
            .as_ref()
            .clone();
        let game = runtime
            .resources
            .get::<Arc<GameIdentity>>()?
            .as_ref()
            .clone();
        let place = runtime
            .resources
            .get::<ProfilePlace>()
            .copied()
            .unwrap_or(ProfilePlace::Developer);
        let path = profile_path(place, &machine, &game, &|key| std::env::var_os(key));

        let local = (path.as_deref())
            .and_then(read)
            .filter(|profile| fits(profile, &game, Some(&machine)));
        let profile = match (local, place) {
            (Some(local), _) => Some(local),
            (None, ProfilePlace::Player) => (runtime.resources.get::<ShippedProfileDir>())
                .and_then(|shipped| read(&shipped.0.join(SHIPPED_PROFILE)))
                .filter(|profile| fits(profile, &game, None)),
            (None, ProfilePlace::Developer) => None,
        };
        if let Some(profile) = &profile {
            let rows = profile.sane_rows();
            log::info!(
                "Machine profile: {} rows learnt on an earlier run seed the DCC",
                rows.len()
            );
            dcc.seed_cost_book(rows);
        }
        // With no folder to file this machine's profile in, nothing is
        // written; what the game shipped still seeds it.
        Some(Self {
            path: path?,
            machine,
            game,
            saved: Vec::new(),
            saved_at: Instant::now(),
        })
    }

    /// At a frame's end: writes what the DCC learnt, when it learnt
    /// something since the last write and [`SAVE_EVERY`] has passed.
    pub(crate) fn tick(&mut self, dcc: &DccService) {
        if self.saved_at.elapsed() < SAVE_EVERY {
            return;
        }
        self.saved_at = Instant::now();
        let rows = dcc.learnt_costs();
        if !rows.is_empty() && rows != self.saved {
            self.write(rows);
        }
    }

    /// At shutdown, the DCC stopped: writes what it learnt last.
    pub(crate) fn end(mut self, dcc: &DccService) {
        let rows = dcc.learnt_costs();
        if !rows.is_empty() {
            self.write(rows);
        }
    }

    fn write(&mut self, rows: Vec<ProfileRow>) {
        let profile = CostProfile {
            engine_version: crate::ENGINE_VERSION.to_owned(),
            machine: self.machine.clone(),
            game: self.game.clone(),
            rows,
        };
        match save(&self.path, &profile) {
            Ok(()) => {
                log::debug!("Machine profile written to {}", self.path.display());
                self.saved = profile.rows;
            }
            Err(error) => log::warn!(
                "Machine profile: could not write {}: {error}",
                self.path.display()
            ),
        }
    }
}

/// The profile at `path`, when there is one that reads.
fn read(path: &std::path::Path) -> Option<CostProfile> {
    match load(path) {
        Ok(profile) => profile,
        Err(error) => {
            log::warn!("Machine profile: {} is not read: {error}", path.display());
            None
        }
    }
}

/// Whether `profile` was learnt by this engine version running `game` —
/// and on `machine`, when one is named.
fn fits(profile: &CostProfile, game: &GameIdentity, machine: Option<&MachineIdentity>) -> bool {
    let fits = profile.engine_version == crate::ENGINE_VERSION
        && profile.game == *game
        && machine.is_none_or(|machine| profile.machine.key() == machine.key());
    if !fits {
        log::info!(
            "Machine profile: one learnt by engine {} running {} {} is left",
            profile.engine_version,
            profile.game.name,
            profile.game.version
        );
    }
    fits
}

#[cfg(test)]
mod tests;
