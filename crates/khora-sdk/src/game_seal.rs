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

//! The seal a game starts under: where its binary sits and the key stamped into it.

use std::path::{Path, PathBuf};

use khora_io::asset::pack_key::stamped_pack_key;
use khora_io::asset::signing::PackKey;

use crate::game_assets::check_pack_seal;

/// Where a game's binary sits and the key stamped into it: what its pack is checked against.
#[derive(Debug, Clone)]
pub(crate) struct GameSeal {
    /// The binary's folder; `None` when it cannot be read.
    exe_dir: Option<PathBuf>,
    key: Option<PackKey>,
}

impl GameSeal {
    /// Returns the seal of the binary in `exe_dir`, stamped with `key`.
    #[cfg(test)]
    pub(crate) fn new(exe_dir: &Path, key: Option<PackKey>) -> Self {
        Self {
            exe_dir: Some(exe_dir.to_path_buf()),
            key,
        }
    }

    /// Returns the seal of the running binary: its folder and the key stamped into it.
    pub(crate) fn of_this_binary() -> Self {
        Self {
            exe_dir: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf)),
            key: stamped_pack_key(),
        }
    }

    /// Refuses to start the game when its pack does not verify, as [`check_pack_seal`] checks it.
    ///
    /// A binary with no key and no pack beside it — a tool, a test, a dev run —
    /// passes.
    ///
    /// # Panics
    ///
    /// When the pack beside the binary does not verify with its key, when a
    /// pack sits beside a binary with no key, when a stamped binary finds no
    /// complete pack, or cannot find its own folder.
    pub(crate) fn enforce(&self) {
        let Some(exe_dir) = &self.exe_dir else {
            assert!(
                self.key.is_none(),
                "this built game cannot find the folder it runs from: its pack cannot be verified"
            );
            return;
        };
        if let Err(error) = check_pack_seal(exe_dir, self.key) {
            panic!(
                "the game beside {} does not start: {error:#}",
                exe_dir.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use khora_core::Runtime;

    use super::GameSeal;
    use crate::run_default::tests::{opening, GameDir, SEALED};
    use crate::run_default::DefaultRuntimeApp;
    use crate::EngineCore;

    /// **An engine start refuses an unsigned pack**, as `run_winit` boots it.
    ///
    /// The pack beside the binary has no `manifest.bin`: the binary's key verifies
    /// nothing, and the engine does not start.
    #[test]
    #[should_panic(expected = "no manifest.bin")]
    fn an_engine_start_refuses_an_unsigned_pack() {
        let game = GameDir::new("start-unsigned");
        let exe_dir = game.packed_unsigned(SEALED);
        let key = game.pack_key();

        let mut engine = EngineCore::<DefaultRuntimeApp>::new();
        engine.bootstrap_sealed(
            DefaultRuntimeApp::at(exe_dir.clone(), opening("scenes/missing.kscene")),
            Runtime::new(),
            &GameSeal::new(&exe_dir, Some(key)),
        );
    }
}
