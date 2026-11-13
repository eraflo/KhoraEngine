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

//! The engine, simulated: agents of quoted rows whose true cost differs
//! from their quotes, a host part, the frame governor, the arbiter and the
//! DCC's 50 ms tick, in frame time. No thread, no wall clock.

use std::time::{Duration, Instant};

use khora_core::agent::gorna::protocol::{
    Allocation, CostBasis, Epoch, Offer, OfferRow, Report, Trigger,
};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, StrategyId};
use khora_core::agent::AgentImportance;
use khora_core::lane::cost::{Cost, PerDevice, PerLink};

use super::Arbiters;
use crate::cost_model::ComplexityClass;
use crate::dcc_service::FRAME_TIME_MIN_SAMPLES;
use crate::gorna::arbiter::Arbiter;
use crate::gorna::cost_book::{correct_bid, lend_in_quotes, CostBook, CostBookConfig};
use crate::gorna::planning::{Planner, PlanningConfig};
use crate::gorna::{Bid, Round};
use crate::governor::{FrameGovernor, GovernorConfig};

/// An agent of the simulation.
#[derive(Debug, Clone)]
pub struct SimAgent {
    /// Its name.
    pub name: &'static str,
    /// `(id, quoted µs on the host, quality)`, floor first.
    pub rows: Vec<(StrategyId, u64, f32)>,
    /// True cost ÷ quote, per row (empty = 1.0 each).
    pub bias: Vec<f64>,
    /// The µs a switch to each row takes to prepare (empty = 2 000 each).
    pub prepare: Vec<u64>,
}

impl SimAgent {
    /// `name`, offering `rows` that cost what they quote.
    pub fn new(name: &'static str, rows: Vec<(StrategyId, u64, f32)>) -> Self {
        Self {
            name,
            rows,
            bias: Vec::new(),
            prepare: Vec::new(),
        }
    }
}

/// A decision of the governor, and the epoch it opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimDecision {
    /// The frame time it was made at.
    pub at: Duration,
    /// The multiplier decided.
    pub multiplier: f32,
    /// Whether the allocation left a lever: some agent above its floor.
    pub lever: bool,
}

/// One frame of the simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimFrame {
    /// The frame time it started at.
    pub at: Duration,
    /// How long it lasted.
    pub time: Duration,
}

/// What a simulation measures.
#[derive(Debug, Clone, PartialEq)]
pub struct SimMetrics {
    /// Frames run.
    pub frames: u64,
    /// Frames over the setpoint.
    pub frames_over: u64,
    /// Frames over the setpoint once the engine runs on what it learnt:
    /// not those before the governor's first decision, nor those within
    /// half a second of a load's step (a change of more than a tenth at
    /// once) — the governor's to answer.
    pub frames_over_steady: u64,
    /// The longest frame.
    pub worst_frame: Duration,
    /// The mean weighed quality of the rows run.
    pub quality_mean: f64,
    /// Row changes per minute of frame time.
    pub row_changes_per_minute: f64,
    /// Mean |quote − true cost| / true cost of the rows run.
    pub prediction_error: f64,
    /// The mean time a decision took, on the wall clock.
    pub decision_time_mean: Duration,
    /// The longest time a decision took, on the wall clock.
    pub decision_time_max: Duration,
    /// How many plans the shield refused.
    pub refusals: usize,
    /// How many decisions were made.
    pub decisions: usize,
}

/// The engine, simulated.
pub struct Simulation {
    governor: FrameGovernor,
    arbiter: Arbiter,
    /// What the planned arbiter learns between rounds; `None` for a set
    /// that decides the frame alone.
    planner: Option<Planner>,
    /// What the planned set learns of each row's measured time against its
    /// quote; `None` for a set that plans on the quotes.
    book: Option<CostBook>,
    /// The epoch of the allocation last lent: the rows run under it.
    lent: Epoch,
    agents: Vec<SimAgent>,
    /// The frame's time outside every agent.
    host: Duration,
    /// What multiplies each agent's true cost.
    loads: Vec<f64>,
    /// How much a true cost varies from one frame to the next, relative.
    noise: f64,
    /// The noise's generator.
    random: u64,
    /// The latest allocation decided: the one in force at the DCC.
    decided: Allocation,
    /// An allocation published, lent at the next frame start.
    published: Option<Allocation>,
    /// The rows chosen at the last lend: they run from the next frame on.
    chosen: Vec<usize>,
    /// The rows the frames run.
    running: Vec<usize>,
    hitch: Option<Duration>,
    clock: Duration,
    epoch: u64,
    decisions: Vec<SimDecision>,
    /// When a load stepped.
    load_changes: Vec<Duration>,
    frames: Vec<SimFrame>,
    switches: Vec<(Duration, usize)>,
    /// How long each round took to decide, on the wall clock.
    decision_times: Vec<Duration>,
    refusals: usize,
    /// The weighed quality of the rows each frame ran, summed.
    quality: f64,
    /// The quotes' relative error against the true costs of the rows run,
    /// summed, and how many were summed.
    error: (f64, u64),
}

impl Simulation {
    /// The engine with `agents` and a host part of `host`, deciding through
    /// `arbiters`, its noise drawn from `seed`. Its first epoch is decided at
    /// the governor's resting multiplier and runs from the first frame.
    pub fn new(
        cfg: GovernorConfig,
        agents: Vec<SimAgent>,
        host: Duration,
        arbiters: Arbiters,
        seed: u64,
    ) -> Self {
        let mut sim = Self {
            governor: FrameGovernor::new(cfg),
            arbiter: arbiters.arbiter(),
            // Each agent's true cost grows with its load, linearly.
            planner: matches!(arbiters, Arbiters::Planned | Arbiters::PlannedOnQuotes).then(|| {
                let mut planner = Planner::new(PlanningConfig::default());
                for agent in &agents {
                    planner.observe_class(AgentId::new(agent.name), ComplexityClass::Linear);
                }
                planner
            }),
            book: (arbiters == Arbiters::Planned).then(|| CostBook::new(CostBookConfig::default())),
            lent: Epoch(0),
            loads: vec![1.0; agents.len()],
            agents,
            host,
            noise: 0.0,
            random: scramble(seed),
            decided: Allocation {
                epoch: Epoch(0),
                trigger: Trigger::Initial,
                envelopes: Vec::new(),
                pins: Vec::new(),
                allowed: Vec::new(),
                price: None,
                lever: false,
                late: Vec::new(),
            },
            published: None,
            chosen: Vec::new(),
            running: Vec::new(),
            hitch: None,
            clock: Duration::ZERO,
            epoch: 0,
            decisions: Vec::new(),
            load_changes: Vec::new(),
            frames: Vec::new(),
            switches: Vec::new(),
            decision_times: Vec::new(),
            refusals: 0,
            quality: 0.0,
            error: (0.0, 0),
        };
        let first = sim.allocate(Trigger::Initial, sim.governor.multiplier());
        sim.chosen = (sim.agents.iter().zip(&sim.loads))
            .map(|(agent, &load)| agent.runs(&first, load))
            .collect();
        sim.running = sim.chosen.clone();
        sim.lent = first.epoch;
        sim.decided = first;
        sim
    }

    /// The same engine whose true costs vary by `noise`, relative: each
    /// frame, each cost is drawn within ±`noise` of its mean.
    pub fn with_noise(mut self, noise: f64) -> Self {
        self.noise = noise.max(0.0);
        self
    }

    /// The frame's time outside every agent, from the next frame on.
    pub fn set_host(&mut self, host: Duration) {
        self.host = host;
    }

    /// Multiplies agent `agent`'s true cost by `load`, from the next frame
    /// on. An agent the simulation does not hold is ignored.
    pub fn set_load(&mut self, agent: usize, load: f64) {
        if let Some(slot) = self.loads.get_mut(agent) {
            let load = load.max(0.0);
            // A step, not a ramp: the load moves by more than a tenth at
            // once.
            if (load - *slot).abs() > 0.1 * *slot {
                self.load_changes.push(self.clock);
            }
            *slot = load;
        }
    }

    /// The next frame lasts `frame`.
    pub fn hitch(&mut self, frame: Duration) {
        self.hitch = Some(frame);
    }

    /// Runs for `secs` seconds of frame time, a DCC tick at a time.
    pub fn run_for(&mut self, secs: f64) {
        let until = self.clock + Duration::from_secs_f64(secs.max(0.0));
        while self.clock < until {
            self.tick();
        }
    }

    /// The frame time elapsed.
    pub fn clock(&self) -> Duration {
        self.clock
    }

    /// The governor's multiplier.
    pub fn multiplier(&self) -> f32 {
        self.governor.multiplier()
    }

    /// Every decision of the governor so far.
    pub fn decisions(&self) -> &[SimDecision] {
        &self.decisions
    }

    /// Every frame so far.
    pub fn frames(&self) -> &[SimFrame] {
        &self.frames
    }

    /// Each row change: when, and which agent.
    pub fn switches(&self) -> &[(Duration, usize)] {
        &self.switches
    }

    /// What the run so far measures.
    pub fn metrics(&self) -> SimMetrics {
        let frames = self.frames.len() as u64;
        let minutes = self.clock.as_secs_f64() / 60.0;
        let decided = self.decision_times.len();
        SimMetrics {
            frames,
            frames_over: (self.frames.iter())
                .filter(|frame| frame.time > setpoint())
                .count() as u64,
            frames_over_steady: (self.frames.iter())
                .filter(|frame| frame.time > setpoint())
                .filter(|frame| {
                    self.decisions
                        .first()
                        .is_some_and(|first| frame.at >= first.at)
                })
                .filter(|frame| {
                    !(self.load_changes.iter()).any(|changed| {
                        frame.at >= *changed && frame.at < *changed + LOAD_STEP_ANSWER
                    })
                })
                .count() as u64,
            worst_frame: (self.frames.iter())
                .map(|frame| frame.time)
                .max()
                .unwrap_or_default(),
            quality_mean: if frames == 0 {
                0.0
            } else {
                self.quality / frames as f64
            },
            row_changes_per_minute: if minutes > 0.0 {
                self.switches.len() as f64 / minutes
            } else {
                0.0
            },
            prediction_error: if self.error.1 == 0 {
                0.0
            } else {
                self.error.0 / self.error.1 as f64
            },
            decision_time_mean: (self.decision_times.iter())
                .sum::<Duration>()
                .checked_div(decided as u32)
                .unwrap_or_default(),
            decision_time_max: (self.decision_times.iter())
                .copied()
                .max()
                .unwrap_or_default(),
            refusals: self.refusals,
            decisions: decided,
        }
    }

    /// The allocation the arbiter decides for a target of `multiplier` × the
    /// setpoint.
    fn allocate(&mut self, trigger: Trigger, multiplier: f32) -> Allocation {
        self.epoch += 1;
        let epoch = Epoch(self.epoch);
        let mut bids: Vec<Bid> = (self.agents.iter().enumerate())
            .map(|(index, agent)| {
                agent.bid(
                    epoch,
                    self.running.get(index).copied().unwrap_or(0),
                    self.loads[index],
                )
            })
            .collect();
        let offers: Vec<Offer> = bids.iter().map(|bid| bid.offer.clone()).collect();
        let planning = (self.planner.as_ref())
            .map(|planner| planner.planning(&offers, epoch))
            .unwrap_or_default();
        // The planned set plans each row at the time the book learnt it
        // takes; the others at its quote.
        let corrections = (self.book.as_ref())
            .map(|book| book.corrections(&offers))
            .unwrap_or_default();
        let caution = planning.caution;
        for bid in &mut bids {
            correct_bid(bid, &corrections, caution);
        }
        let round = Round {
            epoch,
            trigger,
            target: setpoint().mul_f64(f64::from(multiplier)),
            memory: PerDevice::default(),
            link_rate: PerLink::default(),
            waves: &[],
            background: &[],
            bids: &bids,
            planning,
        };
        let started = Instant::now();
        let mut decision = self.arbiter.decide(&round);
        // Lent in the agents' quoted units, held to the plan at their quotes.
        lend_in_quotes(
            &mut decision.allocation,
            &offers,
            &bids,
            &decision.rows,
            &decision.kept,
            &[],
            &corrections,
            caution,
        );
        self.decision_times.push(started.elapsed());
        self.refusals += decision.refused.len();
        decision.allocation
    }

    /// One frame: the rows chosen at the previous lend run, the allocation
    /// published since is lent, and the governor observes the frame.
    fn frame(&mut self) -> Duration {
        let mut switched = vec![false; self.agents.len()];
        for (agent, (&running, &chosen)) in self.running.iter().zip(&self.chosen).enumerate() {
            if running != chosen {
                self.switches.push((self.clock, agent));
                switched[agent] = true;
            }
        }
        self.running = self.chosen.clone();
        if let Some(allocation) = self.published.take() {
            self.lent = allocation.epoch;
            self.chosen = (self.agents.iter().zip(&self.loads))
                .map(|(agent, &load)| agent.runs(&allocation, load))
                .collect();
        }
        let mut agents_time = Duration::ZERO;
        let mut reports = Vec::with_capacity(self.agents.len());
        for (index, switched) in switched.into_iter().enumerate() {
            let row = self.running[index];
            let agent = &self.agents[index];
            // The lane's quote follows its load, as its cost() follows its
            // workload; what it truly takes is that, biased, and drawn.
            let quote = agent.quote(row).as_secs_f64() * self.loads[index];
            let mean = quote * agent.bias(row);
            let drawn = mean * (1.0 + self.noise * uniform(&mut self.random));
            let truth = Duration::from_secs_f64(drawn.max(0.0));
            if !truth.is_zero() {
                let factor = (self.book.as_ref()).map_or(1.0, |book| {
                    book.correction(AgentId::new(agent.name), agent.rows[row].0)
                        .factor
                });
                self.error.0 += (factor * quote - truth.as_secs_f64()).abs() / truth.as_secs_f64();
                self.error.1 += 1;
            }
            self.quality += WEIGHT * f64::from(agent.rows[row].2);
            agents_time += truth;
            // What the agent reports of its run: the row, its time, its load
            // as its workload, and the preparation on the first run of a row
            // it switched to.
            reports.push(Report {
                agent: AgentId::new(agent.name),
                epoch: self.lent,
                row: agent.rows[row].0,
                measured: PerDevice {
                    cpu: truth,
                    ..PerDevice::default()
                },
                done: 1.0,
                health: 1.0,
                over_budget: false,
                stalled: false,
                workload: Some(quote * 1_000_000.0),
                switch: switched.then(|| agent.prepare(row)),
                message: None,
            });
        }
        // A frame takes some time, however light it is: a tick of frames
        // that took none would never end.
        let time = (self.hitch.take())
            .unwrap_or(self.host + agents_time)
            .max(MIN_FRAME);
        self.governor.observe(time, setpoint());
        if let Some(book) = self.book.as_mut() {
            for report in &reports {
                book.observe_report(report);
            }
        }
        if let Some(planner) = self.planner.as_mut() {
            for report in &reports {
                planner.observe_report(report, self.clock);
            }
            planner.observe_frame(time);
        }
        self.frames.push(SimFrame {
            at: self.clock,
            time,
        });
        self.clock += time;
        time
    }

    /// One DCC tick: frames for 50 ms of frame time (one at least), then a
    /// decision once the DCC has seen enough frames; a change opens an epoch.
    fn tick(&mut self) {
        let mut ticked = Duration::ZERO;
        while ticked < TICK {
            ticked += self.frame();
        }
        if self.frames.len() < FRAME_TIME_MIN_SAMPLES {
            return;
        }
        let no_lever = !self.decided.lever;
        if let Some(multiplier) = self.governor.decide(setpoint(), no_lever) {
            let allocation = self.allocate(Trigger::FrameGovernor, multiplier);
            self.decisions.push(SimDecision {
                at: self.clock,
                multiplier,
                lever: allocation.lever,
            });
            self.decided = allocation.clone();
            self.published = Some(allocation);
        }
    }
}

impl SimAgent {
    /// This agent's bid for `epoch` under `load`: its rows at their quotes,
    /// which follow the load, `Important`, learning.
    fn bid(&self, epoch: Epoch, running: usize, load: f64) -> Bid {
        Bid {
            offer: Offer {
                agent: AgentId::new(self.name),
                epoch,
                rows: (self.rows.iter())
                    .map(|&(id, cpu_us, quality)| OfferRow {
                        id,
                        cost: Cost {
                            time: PerDevice {
                                cpu: Duration::from_secs_f64(cpu_us as f64 * load / 1_000_000.0),
                                ..PerDevice::default()
                            },
                            ..Cost::default()
                        },
                        quality,
                        confidence: CostBasis::Measured,
                    })
                    .collect(),
                floor: self.rows[0].0,
                current: self.rows[running.min(self.rows.len() - 1)].0,
                health: 1.0,
            },
            importance: AgentImportance::Important,
            mode: AdaptationMode::Learning,
            hints: AgentHints::default(),
            kept: None,
            late: false,
        }
    }

    /// The row this agent runs under `allocation` and `load`: the best
    /// quality whose quote its envelope's host time fits, else its floor.
    fn runs(&self, allocation: &Allocation, load: f64) -> usize {
        let lent = (allocation.envelopes.iter())
            .find(|(id, _)| *id == AgentId::new(self.name))
            .map_or(Duration::ZERO, |(_, envelope)| envelope.time.cpu);
        (0..self.rows.len())
            .filter(|&row| self.quote(row).as_secs_f64() * load <= lent.as_secs_f64())
            .max_by(|&a, &b| self.rows[a].2.total_cmp(&self.rows[b].2))
            .unwrap_or(0)
    }

    /// How long a switch to row `row` takes to prepare.
    fn prepare(&self, row: usize) -> Duration {
        Duration::from_micros(self.prepare.get(row).copied().unwrap_or(DEFAULT_PREPARE))
    }

    /// What row `row` quotes.
    fn quote(&self, row: usize) -> Duration {
        Duration::from_micros(self.rows[row].1)
    }

    /// What row `row` costs for each microsecond it quotes.
    fn bias(&self, row: usize) -> f64 {
        self.bias.get(row).copied().unwrap_or(1.0)
    }
}

/// How long a switch of row takes to prepare, when the agent does not say.
const DEFAULT_PREPARE: u64 = 2_000;

/// How long the governor is given to answer a change of load.
const LOAD_STEP_ANSWER: Duration = Duration::from_millis(500);

/// The shortest frame.
const MIN_FRAME: Duration = Duration::from_micros(1);

/// The weight of an `Important` agent, as the arbiter weighs it.
const WEIGHT: f64 = 0.75;

/// The governor's setpoint: a 60 Hz frame.
fn setpoint() -> Duration {
    Duration::from_secs_f64(1.0 / 60.0)
}

/// The DCC's tick, in frame time: 20 Hz.
const TICK: Duration = Duration::from_millis(50);

/// `seed` spread over the generator's bits (SplitMix64), never zero.
fn scramble(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)).max(1)
}

/// The next draw of `state` (xorshift64), uniform in [-1, 1].
fn uniform(state: &mut u64) -> f64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}
