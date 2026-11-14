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

//! What the DCC plans a round for beyond its frame: what a switch of row is
//! charged, the margin kept free of the target, and how each agent's costs
//! are expected to grow over the horizon.

use std::time::Duration;

use std::collections::{HashMap, VecDeque};

use khora_core::agent::gorna::protocol::{Epoch, Offer, Report};
use khora_core::agent::gorna::{AgentId, Planning, StrategyId};
use khora_core::agent::KEEP_EPOCHS;

use crate::cost_model::ComplexityClass;

/// The planner's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanningConfig {
    /// How long a plan is meant to hold.
    pub horizon: Duration,
    /// The transient a switch costs beyond its prepare time.
    pub transient: Duration,
    /// What a switch is taken to cost to prepare when the DCC measured none
    /// of the agent's.
    pub switch_prior: Duration,
    /// The chance a frame may run over the planned target (ε).
    pub risk: f64,
    /// How many frames the spread is read over.
    pub frames: usize,
    /// How many frames the spread waits for: fewer read as no spread, the
    /// margin zero.
    pub min_frames: usize,
    /// How many frames in a row past the target plus the margin make a load.
    ///
    /// The first frame within the target after a load ends it, and the spread
    /// is read afresh from there: a load's frames, a startup's among them, say
    /// nothing of the spread after it.
    pub load_frames: usize,
    /// How many reports a workload trend is read over.
    pub trend: usize,
    /// Whether a round plans each agent's costs to grow as its workload trend
    /// says.
    ///
    /// Off by default: a trend read over a few reports follows a burst as
    /// readily as a real rise, and holds back upgrades it should not.
    pub follow_trends: bool,
}

impl Default for PlanningConfig {
    /// Returns a 5 s horizon, a 250 ms transient, a 5 ms prior and a 5 % risk.
    ///
    /// The spread is read over 120 frames once 10 ran, afresh after a load of
    /// 3 frames; a trend is read over 16 reports, and not followed.
    fn default() -> Self {
        Self {
            horizon: Duration::from_secs(5),
            transient: Duration::from_millis(250),
            switch_prior: Duration::from_millis(5),
            risk: 0.05,
            frames: 120,
            min_frames: 10,
            load_frames: 3,
            trend: 16,
            follow_trends: false,
        }
    }
}

/// Returns the standard normal quantile of 1 − `risk` (z), 0 for risk ≥ 0.5.
///
/// A risk of zero or less reads as the smallest positive one.
pub fn z_of(risk: f64) -> f64 {
    if risk.is_nan() || risk >= 0.5 {
        return 0.0;
    }
    normal_quantile(1.0 - risk, risk.max(f64::MIN_POSITIVE))
}

/// Returns the standard normal quantile of `p` in (0, 1), by Acklam's
/// rational approximation (within 1.2·10⁻⁹ relative).
///
/// `upper` is `1 − p`, given apart so a tail too thin for `p` to hold is
/// still read.
fn normal_quantile(p: f64, upper: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const LOW: f64 = 0.024_25;
    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    if p < LOW {
        tail((-2.0 * p.ln()).sqrt())
    } else if p <= 1.0 - LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        -tail((-2.0 * upper.ln()).sqrt())
    }
}

/// Returns `z_of(risk)` × 1.4826 × the median absolute deviation of `frames`.
///
/// Zero for fewer than 2 frames. The median absolute deviation reads the
/// frames' spread without their hitches: a few frames far off move it little.
pub fn margin(frames: &[Duration], risk: f64) -> Duration {
    if frames.len() < 2 {
        return Duration::ZERO;
    }
    let mut secs: Vec<f64> = frames.iter().map(Duration::as_secs_f64).collect();
    let middle = median(&mut secs);
    let mut deviations: Vec<f64> = secs.iter().map(|x| (x - middle).abs()).collect();
    let spread = 1.4826 * median(&mut deviations);
    let margin = z_of(risk) * spread;
    if margin.is_finite() && margin > 0.0 {
        Duration::from_secs_f64(margin)
    } else {
        Duration::ZERO
    }
}

/// Returns the median of `values`, which it sorts.
fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    let half = values.len() / 2;
    if values.len() % 2 == 1 {
        values[half]
    } else {
        (values[half - 1] + values[half]) / 2.0
    }
}

/// Returns the cost growth over `horizon` of a workload whose recent
/// `(at, n)` samples are `trend`, read through `class`.
///
/// f(n(t+H)) / f(n(t)), n(t+H) from a least-squares line over the samples;
/// 1 with fewer than 2 samples, a flat trend, or f(n(t)) ≤ 0; never negative.
pub fn growth(trend: &[(Duration, f64)], class: ComplexityClass, horizon: Duration) -> f64 {
    if trend.len() < 2 {
        return 1.0;
    }
    let count = trend.len() as f64;
    let mean_t = trend.iter().map(|(at, _)| at.as_secs_f64()).sum::<f64>() / count;
    let mean_n = trend.iter().map(|(_, n)| *n).sum::<f64>() / count;
    let (mut covariance, mut variance) = (0.0, 0.0);
    for (at, n) in trend {
        let dt = at.as_secs_f64() - mean_t;
        covariance += dt * (n - mean_n);
        variance += dt * dt;
    }
    if variance <= 0.0 {
        return 1.0;
    }
    let slope = covariance / variance;
    if slope == 0.0 || !slope.is_finite() {
        return 1.0;
    }
    let last = trend
        .iter()
        .map(|(at, _)| at.as_secs_f64())
        .fold(f64::MIN, f64::max);
    let line = |t: f64| mean_n + slope * (t - mean_t);
    let now = class.f(line(last));
    if now.is_nan() || now <= 0.0 {
        return 1.0;
    }
    let later = class.f(line(last + horizon.as_secs_f64()).max(0.0));
    let growth = later / now;
    if growth.is_finite() {
        growth.max(0.0)
    } else {
        1.0
    }
}

/// What the DCC learns between rounds for its planning.
pub struct Planner {
    config: PlanningConfig,
    /// The last preparation measured for each (agent, row).
    switches: HashMap<(AgentId, StrategyId), Duration>,
    /// The last epoch each agent ran each row under.
    ran: HashMap<AgentId, HashMap<StrategyId, Epoch>>,
    /// Each agent's recent workloads, and when they were reported.
    trends: HashMap<AgentId, VecDeque<(Duration, f64)>>,
    /// The class each agent's cost model fits.
    classes: HashMap<AgentId, ComplexityClass>,
    /// The recent frames' critical paths, since the last load ended.
    frames: VecDeque<Duration>,
    /// How many of the last frames in a row ran past their target.
    over: usize,
}

impl Planner {
    /// Creates a planner of `cfg` that has learnt nothing yet.
    pub fn new(cfg: PlanningConfig) -> Self {
        Self {
            config: cfg,
            switches: HashMap::new(),
            ran: HashMap::new(),
            trends: HashMap::new(),
            classes: HashMap::new(),
            frames: VecDeque::new(),
            over: 0,
        }
    }

    /// Returns the planner's settings.
    pub fn config(&self) -> PlanningConfig {
        self.config
    }

    /// Records a report received at `at`: its switch time when it carries
    /// one, the row it ran under its epoch, its workload.
    pub fn observe_report(&mut self, report: &Report, at: Duration) {
        if let Some(switch) = report.switch {
            self.switches.insert((report.agent, report.row), switch);
        }
        let ran = self
            .ran
            .entry(report.agent)
            .or_default()
            .entry(report.row)
            .or_insert(report.epoch);
        *ran = (*ran).max(report.epoch);
        if let Some(workload) = report.workload.filter(|n| n.is_finite()) {
            let trend = self.trends.entry(report.agent).or_default();
            trend.push_back((at, workload));
            while trend.len() > self.config.trend.max(1) {
                trend.pop_front();
            }
        }
    }

    /// Records a frame's critical path and the target it ran under.
    ///
    /// The first frame within the target after a load (see
    /// [`PlanningConfig::load_frames`]) restarts the frames the spread is
    /// read over.
    pub fn observe_frame(&mut self, critical: Duration, target: Duration) {
        // A frame past the target but within the margin neither counts
        // toward a load nor ends one.
        if critical > target.saturating_add(self.margin()) {
            self.over = self.over.saturating_add(1);
        } else if critical <= target {
            if self.over >= self.config.load_frames.max(1) {
                self.frames.clear();
            }
            self.over = 0;
        }
        self.frames.push_back(critical);
        while self.frames.len() > self.config.frames.max(1) {
            self.frames.pop_front();
        }
    }

    /// Returns the margin of the frames since the last load ended: none
    /// below `min_frames` of them.
    fn margin(&self) -> Duration {
        if self.frames.len() < self.config.min_frames {
            return Duration::ZERO;
        }
        let frames: Vec<Duration> = self.frames.iter().copied().collect();
        margin(&frames, self.config.risk)
    }

    /// Records the class `agent`'s cost model fits.
    pub fn observe_class(&mut self, agent: AgentId, class: ComplexityClass) {
        self.classes.insert(agent, class);
    }

    /// Returns the planning for a round of `epoch` over `offers`.
    ///
    /// A row other than the offer's `current` is charged a switch time: none
    /// when the agent ran it within the last `KEEP_EPOCHS` epochs (its layer
    /// is kept), else the last measured for it, else the agent's mean
    /// measured, else `switch_prior`.
    ///
    /// See the book: concepts/gorna.md, "Formalisation".
    pub fn planning(&self, offers: &[Offer], epoch: Epoch) -> Planning {
        let mut planning = Planning {
            horizon: self.config.horizon,
            transient: self.config.transient,
            margin: self.margin(),
            growth: Vec::new(),
            switch: Vec::new(),
            // A row is planned this many spreads above its correction: the
            // same risk the margin keeps.
            caution: z_of(self.config.risk),
        };
        for offer in offers {
            let agent = offer.agent;
            let followed = (self.config.follow_trends)
                .then(|| self.trends.get(&agent).zip(self.classes.get(&agent)))
                .flatten();
            if let Some((trend, class)) = followed {
                let samples: Vec<(Duration, f64)> = trend.iter().copied().collect();
                let growth = growth(&samples, *class, self.config.horizon);
                if growth != 1.0 {
                    planning.growth.push((agent, growth));
                }
            }
            let ran = self.ran.get(&agent);
            let kept = |row: StrategyId| {
                ran.and_then(|ran| ran.get(&row))
                    .is_some_and(|last| epoch.0.saturating_sub(last.0) <= u64::from(KEEP_EPOCHS))
            };
            let measured: Vec<Duration> = (self.switches.iter())
                .filter(|((named, _), _)| *named == agent)
                .map(|(_, time)| *time)
                .collect();
            let mean = (!measured.is_empty())
                .then(|| measured.iter().sum::<Duration>() / measured.len() as u32);
            for row in &offer.rows {
                if row.id == offer.current {
                    continue;
                }
                // A row whose layer is still kept prepares nothing, but the
                // swap still pays its transient.
                let time = if kept(row.id) {
                    Duration::ZERO
                } else {
                    (self.switches.get(&(agent, row.id)).copied())
                        .or(mean)
                        .unwrap_or(self.config.switch_prior)
                };
                planning.switch.push((agent, row.id, time));
            }
        }
        planning
    }
}
