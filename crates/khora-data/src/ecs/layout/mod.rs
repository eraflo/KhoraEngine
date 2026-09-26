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
mod tests {
    use super::*;

    #[test]
    fn explores_every_arm_first() {
        let mut b = Ucb1::new(3);
        // First three selections must each be a distinct, never-pulled arm.
        for _ in 0..3 {
            let arm = b.select();
            assert_eq!(b.pulls(arm), 0);
            b.record(arm, 0.0);
        }
        assert_eq!((0..3).map(|i| b.pulls(i)).sum::<u64>(), 3);
    }

    #[test]
    fn converges_to_best_arm() {
        // Arm 1 is best (reward 1.0); arms 0 and 2 give 0.0.
        let mut b = Ucb1::new(3);
        let reward = |arm: usize| if arm == 1 { 1.0 } else { 0.0 };
        for _ in 0..200 {
            let arm = b.select();
            b.record(arm, reward(arm));
        }
        assert_eq!(b.best_arm(), Some(1));
        // The best arm should dominate the pull budget.
        assert!(
            b.pulls(1) > b.pulls(0) + b.pulls(2),
            "best arm pulled {} vs others {}+{}",
            b.pulls(1),
            b.pulls(0),
            b.pulls(2)
        );
    }

    #[test]
    fn deterministic_same_stream_same_choice() {
        let run = || {
            let mut b = Ucb1::new(4);
            let mut picks = Vec::new();
            for _ in 0..50 {
                let arm = b.select();
                picks.push(arm);
                b.record(arm, (arm as f64) * 0.1);
            }
            picks
        };
        assert_eq!(run(), run(), "UCB1 must be deterministic");
    }

    #[test]
    fn ignores_bad_input() {
        let mut b = Ucb1::new(2);
        b.record(99, 1.0); // out of range
        b.record(0, f64::NAN); // non-finite
        assert_eq!(b.pulls(0), 0);
        assert_eq!(b.best_arm(), None);
    }

    #[test]
    fn sliding_window_explores_then_tracks_a_shifting_best_arm() {
        // Arm 0 is best in "phase 1", arm 1 in "phase 2". A short window must
        // let the bandit follow the shift instead of pinning to arm 0.
        let mut b = SlidingWindowUcb1::new(2, 16);
        let phase1 = |arm: usize| if arm == 0 { 1.0 } else { 0.0 };
        let phase2 = |arm: usize| if arm == 1 { 1.0 } else { 0.0 };

        for _ in 0..100 {
            let arm = b.select();
            b.record(arm, phase1(arm));
        }
        assert_eq!(b.best_arm(), Some(0), "should favour arm 0 in phase 1");

        for _ in 0..100 {
            let arm = b.select();
            b.record(arm, phase2(arm));
        }
        assert_eq!(
            b.best_arm(),
            Some(1),
            "should follow the shift to arm 1 once the window has turned over"
        );
    }

    #[test]
    fn sliding_window_is_deterministic() {
        let run = || {
            let mut b = SlidingWindowUcb1::new(4, 8);
            let mut picks = Vec::new();
            for _ in 0..50 {
                let arm = b.select();
                picks.push(arm);
                b.record(arm, (arm as f64) * 0.1);
            }
            picks
        };
        assert_eq!(run(), run(), "SW-UCB1 must be deterministic");
    }

    #[test]
    fn net_reward_and_hysteresis_gate_switching() {
        // A challenger that beats the incumbent by less than the repack cost
        // must not trigger a switch.
        let incumbent = 1.0;
        let migration_cost = 0.3;
        let marginal_challenger = net_reward(1.2, migration_cost); // 0.9 net
        assert!(!should_switch(incumbent, marginal_challenger, 0.0));
        // A challenger that clears the cost does switch.
        let worthwhile = net_reward(1.8, migration_cost); // 1.5 net
        assert!(should_switch(incumbent, worthwhile, 0.0));
        // The explicit hysteresis margin adds dwell on top.
        assert!(!should_switch(1.0, 1.2, 0.5));
        assert!(should_switch(1.0, 1.6, 0.5));
    }

    #[test]
    fn decay_counter_evaporates_and_favours_recency() {
        let mut c = DecayCounter::new(0.5);
        c.add(1.0);
        c.tick(); // 0.5
        c.tick(); // 0.25
        assert!((c.value() - 0.25).abs() < 1e-9);
        // A fresh deposit dominates the faded history.
        c.add(1.0);
        assert!(c.value() > 1.0 && c.value() < 1.5);

        // Decay is clamped: a >1 factor cannot grow the value unbounded.
        let mut clamped = DecayCounter::new(2.0);
        clamped.add(1.0);
        clamped.tick();
        assert!(clamped.value() <= 1.0);
    }

    #[test]
    fn advisor_maps_access_stats_to_layout() {
        let advisor = LayoutAdvisor::default();
        // Fat component → hot/cold split, whatever the access.
        assert_eq!(
            advisor.recommend(128, 10, 100),
            LayoutRecommendation::HotColdSplit
        );
        // Lean component swept in large batches → field-SoA/SIMD candidate.
        assert_eq!(
            advisor.recommend(40, 60, 60 * 4096),
            LayoutRecommendation::SimdFieldSoa
        );
        // Lean component, small/occasional access → leave it as SoA.
        assert_eq!(advisor.recommend(40, 5, 20), LayoutRecommendation::KeepSoa);
        // No access recorded → no recommendation to change.
        assert_eq!(advisor.recommend(40, 0, 0), LayoutRecommendation::KeepSoa);
    }
}
