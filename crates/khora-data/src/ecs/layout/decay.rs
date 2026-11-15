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

//! An exponentially decaying event counter.

/// An evaporating access tally: added to on access, decaying geometrically each tick.
///
/// A steady stream of accesses holds the value up and a burst that stops fades
/// away, so it tracks current pressure without a window.
#[derive(Debug, Clone, Copy)]
pub struct DecayCounter {
    value: f64,
    decay: f64,
}

impl DecayCounter {
    /// Creates a counter at zero keeping `decay` of its value each tick, clamped to `[0, 1]`.
    pub fn new(decay: f64) -> Self {
        Self {
            value: 0.0,
            decay: decay.clamp(0.0, 1.0),
        }
    }

    /// Deposits `amount` (an access this tick).
    pub fn add(&mut self, amount: f64) {
        if amount.is_finite() {
            self.value += amount;
        }
    }

    /// Evaporates the value by the decay factor — call once per time step.
    pub fn tick(&mut self) {
        self.value *= self.decay;
    }

    /// Returns the current, recency-weighted value.
    pub fn value(&self) -> f64 {
        self.value
    }
}
