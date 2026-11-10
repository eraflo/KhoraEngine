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

//! The evaluation: a journal decided again (`replay.rs`), the simulated
//! engine and its scenarios (`simulation.rs`), and the loop the frame
//! governor closes through GORNA, simulated (`closed_loop.rs`).

mod closed_loop;
mod replay;
mod simulation;

use std::time::Duration;

use super::*;
use crate::governor::GovernorConfig;
use khora_core::agent::gorna::StrategyId;

/// The governor's setpoint: a 60 Hz frame.
fn setpoint() -> Duration {
    Duration::from_secs_f64(1.0 / 60.0)
}

fn ms(ms: f64) -> Duration {
    Duration::from_secs_f64(ms / 1_000.0)
}

/// The engine with `agents` and a host part of `host_ms`, as the engine
/// decides — the Q-RAM pass then the exact search — with no noise.
fn engine(cfg: GovernorConfig, agents: Vec<SimAgent>, host_ms: f64) -> Simulation {
    Simulation::new(cfg, agents, ms(host_ms), Arbiters::GreedyExact, 0)
}
