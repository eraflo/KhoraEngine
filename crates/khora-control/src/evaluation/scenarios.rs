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

//! The scenarios the proposer sets are compared on: seeded and
//! deterministic.

use std::time::Duration;

use khora_core::agent::gorna::StrategyId::{self, Balanced, HighPerformance, LowPower};

use super::{Arbiters, SimAgent, SimMetrics, Simulation};
use crate::governor::GovernorConfig;

/// How often a scenario sets its loads, in seconds of frame time.
const STEP: f64 = 0.25;

/// A scripted run of the simulated engine: its agents, its host part, and
/// how their loads move over time.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// The scenario's name, as a table prints it.
    pub name: &'static str,
    /// How long it runs, in seconds of frame time.
    seconds: f64,
    /// What multiplies agent `agent`'s true cost at `t` seconds:
    /// `load(t, agent)`.
    load: fn(f64, usize) -> f64,
}

/// The agents every scenario runs, in order: renderer, shadows, physics, scripts, audio.
///
/// Like a lane calibrated on another machine, some take a fixed factor of
/// what they quote on every row: the renderer and the scripts more (×1.2,
/// ×1.3), physics less (×0.8). Every
/// agent at its best overruns a 60 Hz frame; every agent at its floor leaves
/// half of it free.
fn agents() -> Vec<SimAgent> {
    let rows = |rows: &[(StrategyId, u64, f32)]| rows.to_vec();
    let biased = |mut agent: SimAgent, bias: f64| {
        agent.bias = vec![bias; agent.rows.len()];
        agent
    };
    vec![
        biased(
            SimAgent::new(
                "render",
                rows(&[
                    (LowPower, 2_000, 0.3),
                    (Balanced, 4_000, 0.7),
                    (HighPerformance, 7_000, 1.0),
                ]),
            ),
            1.2,
        ),
        SimAgent::new(
            "shadow",
            rows(&[
                (LowPower, 500, 0.3),
                (Balanced, 1_500, 0.7),
                (HighPerformance, 3_000, 1.0),
            ]),
        ),
        biased(
            SimAgent::new(
                "physics",
                rows(&[
                    (LowPower, 1_000, 0.4),
                    (Balanced, 2_000, 0.8),
                    (HighPerformance, 3_000, 1.0),
                ]),
            ),
            0.8,
        ),
        biased(
            SimAgent::new(
                "script",
                rows(&[(LowPower, 800, 0.5), (HighPerformance, 2_000, 1.0)]),
            ),
            1.3,
        ),
        SimAgent::new(
            "audio",
            rows(&[(LowPower, 300, 0.5), (HighPerformance, 800, 1.0)]),
        ),
    ]
}

/// The frame's time outside every agent.
const HOST: Duration = Duration::from_millis(3);

/// How much the true costs vary from one frame to the next, relative.
const NOISE: f64 = 0.05;

const RENDER: usize = 0;
const SHADOW: usize = 1;
const SCRIPT: usize = 3;

impl Scenario {
    /// Every scenario: a light ramp, shadow casters, a script load and
    /// thermal throttling.
    pub fn all() -> Vec<Scenario> {
        vec![
            // Lights are added: the renderer's load rises from ×1 to ×3
            // over 30 s, then holds.
            Scenario {
                name: "light ramp",
                seconds: 45.0,
                load: |t, agent| {
                    if agent == RENDER {
                        1.0 + 2.0 * (t / 30.0).min(1.0)
                    } else {
                        1.0
                    }
                },
            },
            // Shadow casters enter the view at 10 s: the shadows cost twice
            // as much from then on.
            Scenario {
                name: "shadow casters",
                seconds: 30.0,
                load: |t, agent| {
                    if agent == SHADOW && t >= 10.0 {
                        2.0
                    } else {
                        1.0
                    }
                },
            },
            // Scripts burst for one second in every five, four times as
            // costly.
            Scenario {
                name: "script load",
                seconds: 30.0,
                load: |t, agent| {
                    if agent == SCRIPT && t % 5.0 < 1.0 {
                        4.0
                    } else {
                        1.0
                    }
                },
            },
            // The machine heats up: every cost rises from ×1 to ×1.4 over
            // 60 s, then holds.
            Scenario {
                name: "thermal",
                seconds: 75.0,
                load: |t, _| 1.0 + 0.4 * (t / 60.0).min(1.0),
            },
        ]
    }

    /// The scenario run through `arbiters`, its noise drawn from `seed`.
    pub fn run(&self, arbiters: Arbiters, seed: u64) -> SimMetrics {
        let agents = agents();
        let count = agents.len();
        let mut sim = Simulation::new(GovernorConfig::default(), agents, HOST, arbiters, seed)
            .with_noise(NOISE);
        while sim.clock().as_secs_f64() < self.seconds {
            let t = sim.clock().as_secs_f64();
            for agent in 0..count {
                sim.set_load(agent, (self.load)(t, agent));
            }
            sim.run_for(STEP);
        }
        sim.metrics()
    }
}

/// `scenario` run through `arbiters`, its noise drawn from `seed`.
pub fn simulate(scenario: &Scenario, arbiters: Arbiters, seed: u64) -> SimMetrics {
    scenario.run(arbiters, seed)
}
