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

//! The script lane's two levels and their prices: the `[Critical]` behaviours'
//! turns, or every due one. A cut defers a turn, never changes what it
//! computes: the critical level gives up only the share of turns it serves.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::{Confidence, Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};
use khora_data::flow::ScriptView;

use super::rate::schedule_of;
use super::runtime::{ScriptRuntime, UNKNOWN_TURN};

/// The level that serves the `[Critical]` behaviours alone.
pub(super) const CRITICAL: usize = 0;

/// The levels, cheapest first: the critical turns, then every due one — the
/// default.
pub(super) const LEVELS: [Level; 2] = [
    Level {
        id: StrategyId::LowPower,
        name: "critical",
        impact: Impact::None,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    },
    Level {
        id: StrategyId::Balanced,
        name: "whole",
        impact: Impact::None,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    },
];

/// What one row a query takes costs, about: its check, its copies and a body
/// of a few statements.
const PER_ROW: Duration = Duration::from_nanos(200);

/// Returns the view's frame in seconds; a frame of no length (the first) is
/// infinite, so every rate-limited behaviour counts as due.
fn frame_of(view: &ScriptView) -> f32 {
    if view.delta_seconds > 0.0 {
        view.delta_seconds
    } else {
        f32::INFINITY
    }
}

/// What the frame's due turns need.
pub(super) struct Need {
    /// Every due turn's fuel.
    pub all: u64,
    /// The `[Critical]` turns' fuel.
    pub critical: u64,
    /// The share of the frame's due turns that are `[Critical]`.
    pub critical_share: f32,
    /// Whether every turn was priced at what one measured.
    pub measured: bool,
}

/// Returns what the frame's due turns need.
///
/// An instance of the view the scripting world has not met yet (arriving this
/// frame) is priced at its behaviour's measured turn, or at a guess.
pub(super) fn need(runtime: &ScriptRuntime, view: &ScriptView) -> Need {
    let frame = frame_of(view);
    let demand = runtime.demand(frame);
    let known = runtime.known_turns();
    let unknown = UNKNOWN_TURN.as_secs_f64() * 1_000.0 * runtime.rate();
    let (mut all, mut critical) = (demand.all as f64, demand.critical as f64);
    let mut measured = runtime.demand_is_measured();
    // The critical share over the view's instances, which are this frame's
    // turns whether the scripting world has met them or not.
    let (mut weighted, mut weighted_critical) = (0.0_f32, 0.0_f32);
    for instance in &view.instances {
        let Some(program) = view.program_of(instance) else {
            continue;
        };
        if runtime.program(&program.module).is_none() {
            continue;
        }
        let schedule = schedule_of(runtime, program);
        let share = schedule
            .rate
            .map_or(1.0, |rate| (rate * frame).clamp(0.0, 1.0));
        let met = runtime
            .instances
            .get(&(instance.entity, program.behavior.clone()));
        if met.is_some_and(|held| held.disabled) {
            continue;
        }
        weighted += share;
        if schedule.critical {
            weighted_critical += share;
        }
        if met.is_some() {
            continue;
        }
        let turn = match known.get(&(program.module.as_str(), program.behavior.as_str())) {
            Some(turn) => *turn,
            None => {
                measured = false;
                unknown
            }
        };
        let fuel = turn * f64::from(share);
        all += fuel;
        if schedule.critical {
            critical += fuel;
        }
    }
    Need {
        all: all.round() as u64,
        critical: critical.round() as u64,
        critical_share: if weighted > 0.0 {
            weighted_critical / weighted
        } else {
            0.0
        },
        measured,
    }
}

/// Returns the price of `level` on this frame's view.
///
/// The run's overhead, the fuel its turns need at the measured rate, and the
/// rows the systems' queries took last frame: a system is one instance however
/// many rows it sweeps.
pub(super) fn price(level: usize, runtime: &ScriptRuntime, view: &ScriptView) -> Cost {
    let need = need(runtime, view);
    let rate = runtime.rate().max(1.0);
    let time = |fuel: u64| Duration::from_secs_f64(fuel as f64 / rate / 1_000.0);
    let swept = runtime.last_report().swept_rows;
    let rows = PER_ROW * u32::try_from(swept).unwrap_or(u32::MAX);
    let (fuel, share) = if level == CRITICAL {
        (need.critical, Some(need.critical_share))
    } else {
        (need.all, None)
    };
    Cost {
        time: PerDevice {
            cpu: runtime.overhead() + time(fuel) + rows,
            ..PerDevice::default()
        },
        // Measured once every due turn is priced at what one measured; a
        // guess while any is not.
        confidence: Confidence(if need.measured { 1.0 } else { 0.5 }),
        share,
        ..Cost::default()
    }
}
