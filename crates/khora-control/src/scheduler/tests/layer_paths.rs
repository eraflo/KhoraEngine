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

//! Layers on every path the scheduler runs an agent by: alone inline, on the
//! worker pool in a concurrent wave, and once per fixed sub-step.

use super::allocations::one_phase;
use super::layers::{reports, scheduling, shared, taken, Atlas, Brush, Painter, Plain, SharedLog};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Contention, ExecutionPhase, ExecutionTiming};
use khora_data::ecs::World;
use std::sync::Arc;
use std::time::Duration;

/// **Layers run on every path**: alone inline, and on the worker pool —
/// serially and in waves, two frames each. In waves, `solo`'s layer locks the
/// atlas `pooled-a`'s reads, so `solo` is a wave of its own and the two
/// others share the next one. Each layer runs once a frame, its report names
/// its row.
#[test]
fn layers_run_on_every_path() {
    let paths = [
        (
            "solo",
            Contention::none().locking::<Atlas>(),
            StrategyId::Custom(1),
            3.0,
        ),
        (
            "pooled-a",
            Contention::none().reading::<Atlas>(),
            StrategyId::Custom(3),
            2.0,
        ),
        ("pooled-b", Contention::none(), StrategyId::Custom(4), 1.0),
    ];
    for parallel in [false, true] {
        let log = SharedLog::default();
        let agents = paths
            .iter()
            .map(|(name, contention, row, priority)| {
                shared(
                    Painter::new(
                        name,
                        Brush::new(name, *row, &log).contending(contention.clone()),
                        &log,
                    )
                    .with_timing(one_phase(*priority)),
                )
            })
            .collect();
        let (mut scheduler, rx) = scheduling(agents);
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();

        for _ in 0..2 {
            scheduler.run_frame(&mut world, Arc::new(khora_core::Runtime::new()));
        }

        let done = taken(&log);
        let published = reports(&rx);
        for &(name, _, row, _) in &paths {
            let runs = (done.ran.iter())
                .filter(|&&(agent, ran)| agent == name && ran == row)
                .count();
            assert_eq!(runs, 2, "`{name}`, parallel {parallel}: {:?}", done.ran);
            let reported = (published.iter())
                .filter(|report| report.agent == AgentId::new(name) && report.row == row)
                .count();
            assert_eq!(reported, 2, "`{name}`, parallel {parallel}: {published:?}");
        }
    }
}

/// A timing that runs in `TRANSFORM`.
fn transform() -> ExecutionTiming {
    ExecutionTiming {
        phase: ExecutionPhase::TRANSFORM,
        ..ExecutionTiming::default()
    }
}

/// The fixed step both agents' places hold.
const STEP: Duration = Duration::from_millis(4);

/// **A fixed-step family runs its layer once per sub-step.** Beside an agent
/// installed from the start at the same step, the agent installing its layer
/// on its first wake runs it exactly as many times in the frame as the other
/// — more than once — serially and in waves.
#[test]
fn a_fixed_step_family_runs_its_layer_once_per_sub_step() {
    for parallel in [false, true] {
        let log = SharedLog::default();
        let metronome = shared(
            Plain::new("metronome", &log)
                .with_timing(transform())
                .stepping(STEP),
        );
        let painter = shared(
            Painter::new(
                "painter",
                Brush::new("painter", StrategyId::Balanced, &log),
                &log,
            )
            .with_timing(transform())
            .stepping(STEP),
        );
        let (mut scheduler, _rx) = scheduling(vec![metronome, painter]);
        scheduler.set_parallel_execution(parallel);
        let runtime = Arc::new(khora_core::Runtime::new());

        scheduler.run_frame(&mut World::new(), runtime);

        let done = taken(&log);
        let ticks = done
            .ran
            .iter()
            .filter(|(name, _)| *name == "metronome")
            .count();
        let layer_runs = done
            .ran
            .iter()
            .filter(|(name, _)| *name == "painter")
            .count();
        assert!(ticks > 1, "the frame sub-steps more than once: {ticks}");
        assert_eq!(layer_runs, ticks, "parallel {parallel}: once per sub-step");
    }
}
