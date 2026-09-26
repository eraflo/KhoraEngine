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

//! Fitting the negotiated strategies into the frame budget: the greedy fit,
//! and the per-agent clamps that adaptation modes and hints apply.

use super::{strategy_rank, AgentAllocation, AgentNegotiation, GornaArbitrator};
use khora_core::control::gorna::{AgentHints, StrategyId, StrategyOption};

impl GornaArbitrator {
    /// Runs the global budget fitting algorithm.
    ///
    /// Strategy: Priority-weighted greedy allocation.
    /// 1. Sort agents by priority (highest first).
    /// 2. Try to give each agent its most expensive strategy that fits.
    /// 3. If the total exceeds the budget, downgrade lower-priority agents first.
    /// 4. Respect VRAM constraints if specified.
    ///
    /// Time is costed along the **critical path**: agents grouped in the same
    /// wave by `self.wave_plan` run concurrently, so the wave contributes only
    /// its `max` member time to the frame, and the frame budget is spent against
    /// the sum over waves. An empty plan (serial execution) makes every agent
    /// its own wave, so this reduces exactly to the sum-of-costs fit. VRAM is
    /// always additive (memory does not overlap), so it stays a plain sum.
    pub(super) fn fit_budgets(
        &self,
        negotiations: &[AgentNegotiation],
        total_budget_ms: f32,
        max_vram_bytes: Option<u64>,
    ) -> Vec<AgentAllocation> {
        if negotiations.is_empty() {
            return Vec::new();
        }

        let mut sorted_indices: Vec<usize> = (0..negotiations.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            negotiations[b]
                .priority
                .partial_cmp(&negotiations[a].priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut allocations: Vec<AgentAllocation> = negotiations
            .iter()
            .map(|n| AgentAllocation {
                agent_index: n.agent_index,
                strategy: n.strategies[0].clone(),
            })
            .collect();

        // Assign each negotiation to a wave id. Agents named together in a plan
        // wave share one; any agent absent from the plan (or when the plan is
        // empty) becomes its own singleton wave — so its cost is summed, never
        // hidden under a `max`.
        let wave_of = self.assign_waves(negotiations);
        let wave_count = wave_of.iter().copied().max().map_or(0, |m| m + 1);

        // The frame's baseline time is the sum over waves of each wave's slowest
        // member (its critical path). `wave_max[w]` also stays the invariant
        // "current max member cost of wave w" through the upgrade loop below.
        let cost_ms = |a: &AgentAllocation| a.strategy.estimated_time.as_secs_f32() * 1000.0;
        let mut wave_max = vec![0.0_f32; wave_count];
        for (i, a) in allocations.iter().enumerate() {
            wave_max[wave_of[i]] = wave_max[wave_of[i]].max(cost_ms(a));
        }
        let total_min_ms: f32 = wave_max.iter().sum();

        let total_min_vram: u64 = allocations.iter().map(|a| a.strategy.estimated_vram).sum();

        if total_min_ms > total_budget_ms {
            log::warn!(
                "GORNA: Even minimum strategies ({:.2}ms critical path) exceed budget ({:.2}ms). \
                All agents at LowPower.",
                total_min_ms,
                total_budget_ms
            );
            return allocations;
        }

        if let Some(max_vram) = max_vram_bytes {
            if total_min_vram > max_vram {
                log::warn!(
                    "GORNA: Even minimum strategies VRAM ({:.2}MB) exceeds budget ({:.2}MB).",
                    total_min_vram as f64 / (1024.0 * 1024.0),
                    max_vram as f64 / (1024.0 * 1024.0)
                );
            }
        }

        let mut remaining_ms = total_budget_ms - total_min_ms;
        let mut current_vram = total_min_vram;

        for &idx in &sorted_indices {
            let negotiation = &negotiations[idx];
            let w = wave_of[idx];
            let current_vram_cost = allocations[idx].strategy.estimated_vram;

            let mut best_upgrade: Option<&StrategyOption> = None;
            for strategy in negotiation.strategies.iter().rev() {
                let cost = strategy.estimated_time.as_secs_f32() * 1000.0;
                // Upgrades only raise cost, and `wave_max[w]` already covers this
                // agent's current cost, so the frame grows only if the agent
                // overtakes its wave's slowest member.
                let delta_ms = (cost - wave_max[w]).max(0.0);
                let delta_vram = strategy.estimated_vram.saturating_sub(current_vram_cost);

                let time_fits = delta_ms <= remaining_ms;
                let vram_fits = max_vram_bytes
                    .map(|max| current_vram + delta_vram <= max)
                    .unwrap_or(true);

                if time_fits && vram_fits {
                    best_upgrade = Some(strategy);
                    break;
                }
            }

            if let Some(upgrade) = best_upgrade {
                let new_cost = upgrade.estimated_time.as_secs_f32() * 1000.0;
                let delta_vram = upgrade.estimated_vram.saturating_sub(current_vram_cost);
                let delta_frame = (new_cost - wave_max[w]).max(0.0);

                remaining_ms -= delta_frame;
                wave_max[w] = wave_max[w].max(new_cost);
                current_vram += delta_vram;
                allocations[idx].strategy = upgrade.clone();

                log::trace!(
                    "GORNA: Upgraded {:?} to {:.2}ms (wave {} max={:.2}ms, remaining={:.2}ms, vram={:.2}MB)",
                    negotiation.agent_id,
                    new_cost,
                    w,
                    wave_max[w],
                    remaining_ms,
                    current_vram as f64 / (1024.0 * 1024.0)
                );
            }
        }

        if let Some(max_vram) = max_vram_bytes {
            let total_vram: u64 = allocations.iter().map(|a| a.strategy.estimated_vram).sum();
            log::debug!(
                "GORNA: Total VRAM allocated: {:.2}MB / {:.2}MB",
                total_vram as f64 / (1024.0 * 1024.0),
                max_vram as f64 / (1024.0 * 1024.0)
            );
        }

        allocations
    }

    /// Clamps `fitted` into the `[min, max]` strategy range by picking, from the
    /// agent's offered strategies within range, the one nearest the fit. Honours
    /// `Bounded` mode.
    pub(super) fn clamp_strategy(
        &self,
        negotiations: &[AgentNegotiation],
        agent_index: usize,
        fitted: &StrategyOption,
        min: StrategyId,
        max: StrategyId,
    ) -> StrategyOption {
        let (lo, hi) = (strategy_rank(min), strategy_rank(max));
        let fr = strategy_rank(fitted.id);
        if fr >= lo && fr <= hi {
            return fitted.clone();
        }
        let Some(n) = negotiations.iter().find(|n| n.agent_index == agent_index) else {
            return fitted.clone();
        };
        let mut best: Option<&StrategyOption> = None;
        for s in &n.strategies {
            let sr = strategy_rank(s.id);
            if sr < lo || sr > hi {
                continue;
            }
            let closer = match best {
                None => true,
                Some(b) => {
                    (sr as i32 - fr as i32).abs() < (strategy_rank(b.id) as i32 - fr as i32).abs()
                }
            };
            if closer {
                best = Some(s);
            }
        }
        best.cloned().unwrap_or_else(|| fitted.clone())
    }

    /// Clamps `fitted` down to a developer `Cap` hint: if the fitted strategy's
    /// estimated cost exceeds `hints.cap_ms`, returns the most expensive offered
    /// strategy still within the ceiling (or the cheapest, if even that
    /// exceeds it). No cap, or already within it → `fitted` unchanged.
    pub(super) fn apply_cap(
        &self,
        negotiations: &[AgentNegotiation],
        agent_index: usize,
        fitted: StrategyOption,
        hints: Option<&AgentHints>,
    ) -> StrategyOption {
        let Some(max_ms) = hints.and_then(|h| h.cap_ms) else {
            return fitted;
        };
        if fitted.estimated_time.as_secs_f32() * 1000.0 <= max_ms {
            return fitted;
        }
        let Some(n) = negotiations.iter().find(|n| n.agent_index == agent_index) else {
            return fitted;
        };
        // `strategies` is sorted ascending by estimated_time, so the last option
        // within the ceiling is the richest one that honours the cap; if none
        // fit, fall back to the cheapest (index 0) — the closest we can get.
        let mut chosen = &n.strategies[0];
        for s in &n.strategies {
            if s.estimated_time.as_secs_f32() * 1000.0 <= max_ms {
                chosen = s;
            } else {
                break;
            }
        }
        chosen.clone()
    }

    /// Finds the negotiated [`StrategyOption`] with `id` for the agent at
    /// `agent_index`, if that agent offered it. Used to honour `Manual` mode.
    pub(super) fn strategy_for(
        &self,
        negotiations: &[AgentNegotiation],
        agent_index: usize,
        id: StrategyId,
    ) -> Option<StrategyOption> {
        negotiations
            .iter()
            .find(|n| n.agent_index == agent_index)
            .and_then(|n| n.strategies.iter().find(|s| s.id == id).cloned())
    }
}
