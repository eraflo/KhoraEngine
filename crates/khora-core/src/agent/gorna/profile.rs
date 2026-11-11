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

//! What the DCC learnt of a machine running a game: the profile the engine
//! files per machine and game version, and seeds its cost book with at the
//! next boot.

use serde::{Deserialize, Serialize};

use super::StrategyId;
use crate::app::GameIdentity;
use crate::platform::MachineIdentity;

/// One row of what the DCC learnt: an agent's row (by the agent's name), its
/// correction, and the runs it was learnt from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileRow {
    /// The agent's name.
    pub agent: String,
    /// The agent's row.
    pub row: StrategyId,
    /// Measured host time ÷ quoted host time, as learnt.
    pub factor: f64,
    /// How far the runs spread around `factor`.
    pub spread: f64,
    /// How many runs it was learnt from.
    pub runs: u32,
}

/// What the DCC learnt of a machine running a game, filed per machine and
/// game version.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CostProfile {
    /// The version of the engine that learnt it.
    pub engine_version: String,
    /// The machine it was learnt on.
    pub machine: MachineIdentity,
    /// The game it was learnt running.
    pub game: GameIdentity,
    /// What was learnt, row by row.
    pub rows: Vec<ProfileRow>,
}

impl CostProfile {
    /// The rows a profile may seed a book with: factors finite and clamped to
    /// [1e-3, 1e3], spreads finite and ≥ 0 (a NaN, infinite or negative spread
    /// becomes 0), rows with a non-finite factor dropped.
    pub fn sane_rows(&self) -> Vec<ProfileRow> {
        self.rows
            .iter()
            .filter(|row| row.factor.is_finite())
            .map(|row| ProfileRow {
                factor: row.factor.clamp(1e-3, 1e3),
                spread: if row.spread.is_finite() && row.spread > 0.0 {
                    row.spread.min(1e3)
                } else {
                    0.0
                },
                ..row.clone()
            })
            .collect()
    }
}
