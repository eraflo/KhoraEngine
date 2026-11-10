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

//! What a run reports as its workload: the host time the row it ran quoted
//! on that run's own inputs, in µs — the work the lane says it did, which
//! the DCC pairs with the time the run took.

use std::sync::Arc;

use khora_agents::agent::lane_agent;
use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::OutputDeck;
use khora_core::Runtime;

use super::fixtures::{ladder_cost, recorded_level, run, runtime, Plan, LADDER};
use crate::protocol::pin;

/// The host time `level` of the ladder quotes on `load`, in µs.
fn quoted_us(load: u32, level: usize) -> f64 {
    ladder_cost(load, level).time.cpu.as_secs_f64() * 1e6
}

/// **A run reports the host time its row quoted.** The ladder quotes
/// `load × (level + 1)` µs on the host:
/// - pinned to `Balanced` at a load of 100, its run reports a workload of
///   200 µs; the next frame, at a load of 300, 600 µs — the quote follows
///   the run's own inputs;
/// - pinned to `HighPerformance` at a load of 50, 150 µs;
/// - a run whose inputs were refused priced nothing, and reports no
///   workload.
#[test]
fn a_run_reports_the_host_time_its_row_quoted() {
    let mut deck = OutputDeck::new();
    let mut balanced = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut balanced, StrategyId::Balanced);

    let first = run(&mut balanced, 1, &runtime(100, Plan::default()), &mut deck)
        .expect("the pinned row ran");
    assert_eq!(recorded_level(&deck), Some(1), "the Balanced level ran");
    assert_eq!(first.row, StrategyId::Balanced);
    assert_eq!(
        first.workload,
        Some(quoted_us(100, 1)),
        "Balanced quoted 200 µs on its inputs: {first:?}"
    );
    assert_eq!(quoted_us(100, 1), 200.0);

    let second = run(&mut balanced, 2, &runtime(300, Plan::default()), &mut deck)
        .expect("the pinned row ran again");
    assert_eq!(
        second.workload,
        Some(600.0),
        "the quote of the run's own inputs: {second:?}"
    );

    let mut best = lane_agent(&LADDER, &DeviceCaps::default());
    pin(&mut best, StrategyId::HighPerformance);
    let report =
        run(&mut best, 1, &runtime(50, Plan::default()), &mut deck).expect("the pinned row ran");
    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(
        report.workload,
        Some(150.0),
        "HighPerformance quoted 150 µs: {report:?}"
    );

    let refused =
        run(&mut best, 2, &Arc::new(Runtime::new()), &mut deck).expect("the run was reported");
    assert_eq!(
        refused.workload, None,
        "a run that priced nothing names no workload: {refused:?}"
    );
}
