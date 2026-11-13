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

//! Adaptive-layout learner — the decision core of online memory-layout
//! adaptation.
//!
//! Choosing the best physical layout for a component (`Soa` vs an `AoSoA`
//! tiling, …) can't be known a priori: it depends on the access pattern *and*
//! the hardware. So the engine treats it as an explore/exploit problem and
//! **learns** it on the machine it actually runs on — one [`Ucb1`] bandit per
//! component, arms = candidate layouts, reward = measured iteration throughput.
//! `UCB1` is deterministic (no RNG), which keeps decisions reproducible.
//!
//! This module is the learner only. Wiring it to a *physical* repack the query
//! path consumes requires CRPECS storage and `WorldQuery::fetch` to become
//! layout-polymorphic (the column is a type-erased `Vec<T>` today, which only
//! the `Soa` layout fits) — a larger, hot-path change tracked separately. The
//! learner is built and tested now so that work has its decision core ready.
//!
//! Beyond the textbook [`Ucb1`], this module carries the refinements a *game*
//! workload needs (all deterministic, so decisions stay reproducible):
//! [`SlidingWindowUcb1`] for the **non-stationary** reward a session produces
//! (the best layout in a menu is not the best in a firefight), the
//! [`net_reward`]/[`should_switch`] helpers that fold the **repack cost** into
//! the decision so a learner doesn't thrash between layouts, a [`DecayCounter`]
//! (an evaporating access tally — recency-weighting for free), and the
//! read-only [`LayoutAdvisor`] that turns live access counters into actionable
//! layout advice without ever repacking.

mod bandit;
mod decay;

pub use bandit::net_reward;
pub use bandit::should_switch;
pub use bandit::SlidingWindowUcb1;
pub use bandit::Ucb1;
pub use decay::DecayCounter;

/// A layout the [`LayoutAdvisor`] can recommend for a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutRecommendation {
    /// Keep the default `Soa` column — nothing observed warrants a change.
    KeepSoa,
    /// A lean component swept in large batches: a candidate for the field-SoA
    /// `wide::f32x8` kernels *if* its hot loop is compute-bound and stays
    /// SoA-resident (see `khora_core::math::simd`).
    SimdFieldSoa,
    /// A fat component: a candidate for a hot/cold field split so the per-frame
    /// sweep drags only the hot bytes through cache.
    HotColdSplit,
}

impl LayoutRecommendation {
    /// A short, human-readable rationale for the glass-box report.
    pub fn reason(self) -> &'static str {
        match self {
            LayoutRecommendation::KeepSoa => {
                "default SoA is fine — no large-batch or fat-component pressure observed"
            }
            LayoutRecommendation::SimdFieldSoa => {
                "lean component swept in large batches — field-SoA + explicit SIMD if the loop is compute-bound and stays resident"
            }
            LayoutRecommendation::HotColdSplit => {
                "fat component — split hot fields from cold so the sweep touches only hot cache lines"
            }
        }
    }
}

/// Read-only **layout advisor**: turns the live access counters a component has
/// accumulated into an actionable layout suggestion, *without ever repacking*.
///
/// This is the glass-box half of AGDF that works today (the physical repack is
/// deferred): a developer can ask "which of my components would benefit from a
/// different layout, and why?" and get an answer grounded in observed access,
/// not a guess. The thresholds below encode the engine's own benchmark findings
/// (`crates/khora-data/examples/layout_bench.rs`): the field-SoA/SIMD lever pays
/// on *large, compute-bound* batches, and the hot/cold split pays on *fat*
/// components — Khora's lean built-ins want neither. They are deliberately
/// coarse heuristics and meant to be tuned.
#[derive(Debug, Clone, Copy)]
pub struct LayoutAdvisor {
    /// A component at least this large (bytes) is "fat" → hot/cold split.
    pub fat_bytes: usize,
    /// A query touching at least this many rows on average is a "large batch".
    pub large_batch_rows: u64,
}

impl Default for LayoutAdvisor {
    fn default() -> Self {
        // `fat_bytes`: the bench's synthetic fat record (128 B = two cache
        // lines) split 2×; Khora's built-ins (≤64 B) sit below it. `large_batch`:
        // a few SIMD tiles' worth of rows before the field-SoA kernels are worth
        // suggesting.
        Self {
            fat_bytes: 96,
            large_batch_rows: 1024,
        }
    }
}

impl LayoutAdvisor {
    /// Recommends a layout for a component from its size and its
    /// `(query_count, rows_scanned)` access stats (as returned by
    /// `World::component_access_stats`).
    pub fn recommend(
        &self,
        component_bytes: usize,
        query_count: u64,
        rows_scanned: u64,
    ) -> LayoutRecommendation {
        if component_bytes >= self.fat_bytes {
            return LayoutRecommendation::HotColdSplit;
        }
        let avg_rows = rows_scanned.checked_div(query_count).unwrap_or(0);
        if query_count > 0 && avg_rows >= self.large_batch_rows {
            return LayoutRecommendation::SimdFieldSoa;
        }
        LayoutRecommendation::KeepSoa
    }
}

#[cfg(test)]
mod tests;
