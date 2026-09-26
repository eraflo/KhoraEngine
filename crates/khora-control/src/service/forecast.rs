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

//! Forecasting the frame's total cost from the per-agent cost models.

use crate::cost_model::CostModel;

use khora_core::control::gorna::AgentId;
use std::collections::HashMap;

/// Forecasts the frame's empirical cost (`c·f(n)`) at workload `n`, grouping
/// agents into concurrent waves: a wave costs its **critical path** (the `max`
/// of its members), and the frame is the sum over waves.
///
/// With an empty `wave_plan` — serial execution, or before the hot path has
/// published one — every agent is its own singleton wave, so this reduces
/// exactly to the plain per-agent sum. A model for an agent absent from the
/// plan is likewise summed as its own singleton (conservative).
///
/// Returns `None` until at least one agent has enough distinct-`n` samples to
/// fit a model — before that the DCC has nothing to anticipate with.
pub(super) fn forecast_total_ms(
    models: &HashMap<AgentId, CostModel>,
    n: f64,
    wave_plan: &[Vec<AgentId>],
) -> Option<f64> {
    let mut planned: std::collections::HashSet<AgentId> = std::collections::HashSet::new();
    let mut total = 0.0;
    let mut any = false;

    for wave in wave_plan {
        let mut wave_max = 0.0_f64;
        let mut wave_has = false;
        for id in wave {
            planned.insert(*id);
            if let Some(p) = models.get(id).and_then(|m| m.predict_ms(n)) {
                wave_max = wave_max.max(p.max(0.0));
                wave_has = true;
            }
        }
        if wave_has {
            total += wave_max;
            any = true;
        }
    }

    // Any model not covered by the plan is its own singleton wave.
    for (id, m) in models {
        if planned.contains(id) {
            continue;
        }
        if let Some(p) = m.predict_ms(n) {
            total += p.max(0.0);
            any = true;
        }
    }

    any.then_some(total)
}
