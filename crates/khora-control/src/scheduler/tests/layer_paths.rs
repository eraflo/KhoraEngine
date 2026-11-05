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

//! Layers on every path the scheduler runs an agent by: alone with the whole
//! world, inline in a concurrent wave with a shared world, on the worker pool,
//! and once per fixed sub-step.

use super::layers::{reports, scheduling, shared, taken, Brush, Painter, Plain, SharedLog};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{AgentAccess, ExecutionPhase, ExecutionTiming};
use khora_data::ecs::World;
use std::sync::Arc;
use std::time::Duration;

/// **Layers run on every path**: alone with the whole world, inline in a
/// concurrent wave with a shared world, and on the worker pool — serially and
/// in waves, two frames each. Each layer runs once a frame, its report names
/// its row, and no agent is executed.
#[test]
fn layers_run_on_every_path() {
    let paths = [
        ("solo", AgentAccess::Exclusive, StrategyId::Custom(1)),
        ("reader", AgentAccess::SharedWorld, StrategyId::Custom(2)),
        ("pooled-a", AgentAccess::Isolated, StrategyId::Custom(3)),
        ("pooled-b", AgentAccess::Isolated, StrategyId::Custom(4)),
    ];
    for parallel in [false, true] {
        let log = SharedLog::default();
        let agents = paths
            .iter()
            .map(|&(name, access, row)| {
                shared(Painter::new(name, Brush::new(name, row, &log), &log).with_access(access))
            })
            .collect();
        let (mut scheduler, rx) = scheduling(agents);
        scheduler.set_parallel_execution(parallel);
        let mut world = World::new();

        for _ in 0..2 {
            scheduler.run_frame(&mut world, Arc::new(khora_core::Runtime::new()));
        }

        let done = taken(&log);
        assert!(
            done.executed.is_empty(),
            "parallel {parallel}: {:?}",
            done.executed
        );
        let published = reports(&rx);
        for &(name, _, row) in &paths {
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

/// A timing that runs in `TRANSFORM` at a fixed step of 4 ms.
fn fixed_step() -> ExecutionTiming {
    ExecutionTiming {
        allowed_phases: vec![ExecutionPhase::TRANSFORM],
        default_phase: ExecutionPhase::TRANSFORM,
        fixed_timestep: Some(Duration::from_millis(4)),
        ..ExecutionTiming::default()
    }
}

/// **A fixed-step family runs its layer once per sub-step.** Beside an old
/// agent at the same step, the layered agent's layer runs exactly as many
/// times in the frame as the old agent is executed — more than once — and the
/// layered agent itself never is, serially and in waves.
#[test]
fn a_fixed_step_family_runs_its_layer_once_per_sub_step() {
    for (access, parallel) in [
        (AgentAccess::Exclusive, false),
        (AgentAccess::Isolated, true),
    ] {
        let log = SharedLog::default();
        let metronome = shared(Plain::new("metronome", &log).with_timing(fixed_step()));
        let painter = shared(
            Painter::new(
                "painter",
                Brush::new("painter", StrategyId::Balanced, &log),
                &log,
            )
            .with_access(access)
            .with_timing(fixed_step()),
        );
        let (mut scheduler, _rx) = scheduling(vec![metronome, painter]);
        scheduler.set_parallel_execution(parallel);
        let runtime = Arc::new(khora_core::Runtime::new());

        scheduler.run_frame(&mut World::new(), runtime);

        let done = taken(&log);
        let ticks = done
            .executed
            .iter()
            .filter(|name| **name == "metronome")
            .count();
        let layer_runs = done
            .ran
            .iter()
            .filter(|(name, _)| *name == "painter")
            .count();
        assert!(ticks > 1, "the frame sub-steps more than once: {ticks}");
        assert_eq!(
            layer_runs, ticks,
            "{access:?}, parallel {parallel}: once per sub-step"
        );
        assert!(
            !done.executed.contains(&"painter"),
            "{access:?}: the layered agent was executed"
        );
    }
}
