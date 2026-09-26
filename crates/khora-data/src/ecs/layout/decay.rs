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

/// An **evaporating** access tally — a counter that is added to on access and
/// decays geometrically over time.
///
/// This is recency-weighting for free: a steady stream of accesses holds the
/// value up, a burst that stops fades away, so the value tracks *current*
/// pressure without a separate windowing scheme. (Stigmergy: the same mechanic
/// as ant-trail pheromones — deposit on use, evaporate over time — and the
/// discounted-count cousin of [`SlidingWindowUcb1`](super::SlidingWindowUcb1).)
#[derive(Debug, Clone, Copy)]
pub struct DecayCounter {
    value: f64,
    decay: f64,
}

impl DecayCounter {
    /// Creates a counter starting at zero with per-tick retention `decay`,
    /// clamped to `[0, 1]` (e.g. `0.9` keeps 90 % of the value each tick).
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

    /// The current (recency-weighted) value.
    pub fn value(&self) -> f64 {
        self.value
    }
}
