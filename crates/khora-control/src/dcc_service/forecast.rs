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

use khora_core::agent::gorna::AgentId;
use std::collections::HashMap;

/// Forecasts the frame's empirical cost: each wave's slowest member, summed over waves.
///
/// Each agent's model is read at its latest reported workload; an agent that
/// reported none is not counted. An agent absent from `wave_plan` is its own
/// wave, and an agent of `background` runs off the frame and adds nothing.
/// `None` until some agent's model has a fit.
pub(super) fn forecast_frame_ms(
    models: &HashMap<AgentId, CostModel>,
    workloads: &HashMap<AgentId, f64>,
    wave_plan: &[Vec<AgentId>],
    background: &[AgentId],
) -> Option<f64> {
    let predict = |id: &AgentId| {
        let n = *workloads.get(id)?;
        models.get(id)?.predict_ms(n).map(|ms| ms.max(0.0))
    };
    let mut planned = std::collections::HashSet::new();
    let mut total = 0.0;
    let mut any = false;
    for wave in wave_plan {
        let mut slowest: Option<f64> = None;
        for id in wave {
            planned.insert(*id);
            if background.contains(id) {
                continue;
            }
            if let Some(ms) = predict(id) {
                slowest = Some(slowest.map_or(ms, |s: f64| s.max(ms)));
            }
        }
        if let Some(ms) = slowest {
            total += ms;
            any = true;
        }
    }
    // Any model not covered by the plan is its own singleton wave.
    for id in models
        .keys()
        .filter(|id| !planned.contains(*id) && !background.contains(*id))
    {
        if let Some(ms) = predict(id) {
            total += ms;
            any = true;
        }
    }
    any.then_some(total)
}
