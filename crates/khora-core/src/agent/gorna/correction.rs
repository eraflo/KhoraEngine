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

//! What a row's quoted host time is corrected by.

use serde::{Deserialize, Serialize};

/// What a row's host time is corrected by: measured ≈ factor × quoted, and
/// how far its runs spread around that.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    /// Measured host time ÷ quoted host time, as learned.
    pub factor: f64,
    /// How far the runs spread around `factor`, in the same unit.
    pub spread: f64,
    /// How many runs of the row itself it was learnt from; 0 if never measured.
    ///
    /// At 0, `factor` and `spread` are borrowed from the agent's other rows, or
    /// the default. A round plans with those two alone; a journal written
    /// without `runs` reads 0.
    #[serde(default)]
    pub runs: u32,
}

impl Default for Correction {
    /// A row's quote as it is: a factor of 1, no spread, no run.
    fn default() -> Self {
        Self {
            factor: 1.0,
            spread: 0.0,
            runs: 0,
        }
    }
}

impl Correction {
    /// The factor a round plans with: `factor + caution × spread`.
    pub fn planned(&self, caution: f64) -> f64 {
        self.factor + caution * self.spread
    }
}
