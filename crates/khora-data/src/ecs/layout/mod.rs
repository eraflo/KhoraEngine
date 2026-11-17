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

//! The decision core of adaptive memory layout: bandits, switching rules, and an advisor.
//!
//! The [`LayoutAdvisor`] is read-only: none of it repacks storage.
//! See the book: concepts/agdf.md, "The decision core, and why Layer 3 is deferred".

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
    /// A lean component swept in large batches: a candidate for the field-SoA SIMD kernels.
    ///
    /// It pays only if its hot loop is compute-bound and stays SoA-resident
    /// (see `khora_core::math::simd`).
    SimdFieldSoa,
    /// A fat component: a candidate for a hot/cold field split so the per-frame
    /// sweep drags only the hot bytes through cache.
    HotColdSplit,
}

impl LayoutRecommendation {
    /// Returns a short, human-readable rationale for the glass-box report.
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

/// A read-only layout advisor: turns a component's access counters into a layout suggestion.
///
/// It never repacks. Its thresholds are coarse heuristics from
/// `crates/khora-data/examples/layout_bench.rs`, meant to be tuned.
/// See the book: concepts/agdf.md, "Layer 1 — observe and advise".
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
    /// Recommends a layout for a component from its size and its access stats.
    ///
    /// `query_count` and `rows_scanned` are as `World::component_access_stats` returns them.
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
