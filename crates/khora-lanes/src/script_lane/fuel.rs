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

//! What a run costs and what the budget buys: the rate and overhead the lane
//! measures, the fuel a lent time is worth, and the demand the due turns make,
//! each turn priced at what a whole one took.

use std::collections::HashMap;
use std::time::Duration;

use super::runtime::{Demand, Instance, ScriptRuntime, RATE_BLEND, UNKNOWN_TURN};
use super::turn::Outcome;

impl ScriptRuntime {
    /// Corrects the measured rate and overhead from what a run cost: `spent`
    /// fuel in `turns_time` of VM time, within a lane run of `elapsed`.
    ///
    /// The two are measured apart. Timed over the whole run, the rate would
    /// count the frame's bookkeeping as slow instructions: a small fuel would
    /// read as a low rate, buy a smaller fuel the next frame, and the rate
    /// would fall to nothing.
    pub fn observe(&mut self, spent: u64, turns_time: Duration, elapsed: Duration) {
        // Every run says what the bookkeeping cost, one that ran no
        // instruction included.
        let sample = elapsed.saturating_sub(turns_time);
        self.overhead = Some(match self.overhead {
            Some(overhead) => overhead.mul_f64(1.0 - RATE_BLEND) + sample.mul_f64(RATE_BLEND),
            None => sample,
        });

        // Turns too short to time say nothing of the rate: the clock's own
        // resolution would dominate, and a rate read from noise is worse than
        // the last one measured properly.
        let millis = turns_time.as_secs_f64() * 1_000.0;
        if spent == 0 || millis <= f64::EPSILON {
            return;
        }
        let observed = spent as f64 / millis;
        self.rate = self.rate * (1.0 - RATE_BLEND) + observed * RATE_BLEND;
    }

    /// The measured time a lane run costs besides its turns; zero before the
    /// first run.
    pub fn overhead(&self) -> Duration {
        self.overhead.unwrap_or_default()
    }

    /// The fuel `time` buys: what is left of it once the overhead is paid, at
    /// the measured rate. The one definition of a budget's fuel.
    pub fn fuel_for(&self, time: Duration) -> u64 {
        let left = time.saturating_sub(self.overhead());
        (left.as_secs_f64() * 1_000.0 * self.rate) as u64
    }

    /// The fuel the due instances need in a `frame`-long frame: each at
    /// what its last whole turn took — never a piece of one, or a budget cut
    /// would lower the price that buys it back. An instance with no whole
    /// turn yet is priced at what its behavior's other instances took, and
    /// at [`UNKNOWN_TURN`] when none of them has finished one either. A
    /// rate-limited instance counts for the share of frames it runs, as in
    /// [`weighted_instances`](Self::weighted_instances).
    pub fn demand(&self, frame: f32) -> Demand {
        let unknown = UNKNOWN_TURN.as_secs_f64() * 1_000.0 * self.rate;
        let known = self.known_turns();
        // The entities mail waits for: a turn that had nothing to do cost
        // nothing, and says nothing of one that delivers mail. Priced at what
        // it took, an instance whose mail then arrives would be lent no fuel,
        // be deferred, and never measure again.
        let mailed: std::collections::HashSet<_> = self
            .pending
            .as_slice()
            .iter()
            .map(|event| event.target)
            .collect();
        let (mut all, mut critical) = (0.0_f64, 0.0_f64);
        for ((entity, behavior), instance) in &self.instances {
            if instance.disabled {
                continue;
            }
            let schedule = self
                .program(&instance.module)
                .and_then(|program| program.layout(behavior))
                .map(|layout| layout.schedule)
                .unwrap_or_default();
            let share = schedule
                .rate
                .map_or(1.0, |rate| f64::from((rate * frame).clamp(0.0, 1.0)));
            let cost = instance.full_turn.map(|cost| cost as f64).or_else(|| {
                known
                    .get(&(instance.module.as_str(), behavior.as_str()))
                    .copied()
            });
            let cost = match cost {
                Some(cost) if cost <= 0.0 && mailed.contains(entity) => None,
                cost => cost,
            };
            let fuel = cost.unwrap_or(unknown) * share;
            all += fuel;
            if schedule.critical {
                critical += fuel;
            }
        }
        Demand {
            all: all.round() as u64,
            critical: critical.round() as u64,
        }
    }

    /// The share of the due instances that are `[Critical]`, each counted as
    /// in [`demand`](Self::demand): how much of the frame's turns the floor
    /// serves on time. `0.0` with no instance.
    pub fn critical_share(&self, frame: f32) -> f32 {
        let (mut all, mut critical) = (0.0_f32, 0.0_f32);
        for ((_, behavior), instance) in &self.instances {
            if instance.disabled {
                continue;
            }
            let schedule = self
                .program(&instance.module)
                .and_then(|program| program.layout(behavior))
                .map(|layout| layout.schedule)
                .unwrap_or_default();
            let share = schedule
                .rate
                .map_or(1.0, |rate| (rate * frame).clamp(0.0, 1.0));
            all += share;
            if schedule.critical {
                critical += share;
            }
        }
        if all > 0.0 {
            critical / all
        } else {
            0.0
        }
    }

    /// Whether every due instance's price is measured rather than guessed:
    /// it, or another instance of its behavior, has run a whole turn.
    pub fn demand_is_measured(&self) -> bool {
        let known = self.known_turns();
        self.instances
            .iter()
            .filter(|(_, instance)| !instance.disabled)
            .all(|((_, behavior), instance)| {
                instance.full_turn.is_some()
                    || known.contains_key(&(instance.module.as_str(), behavior.as_str()))
            })
    }

    /// The mean whole turn of each behavior, by module and name, over the
    /// instances that have finished one.
    fn known_turns(&self) -> HashMap<(&str, &str), f64> {
        let mut sums: HashMap<(&str, &str), (f64, u32)> = HashMap::new();
        for ((_, behavior), instance) in &self.instances {
            if let (false, Some(cost)) = (instance.disabled, instance.full_turn) {
                let sum = sums
                    .entry((instance.module.as_str(), behavior.as_str()))
                    .or_default();
                sum.0 += cost as f64;
                sum.1 += 1;
            }
        }
        sums.into_iter()
            .map(|(key, (sum, count))| (key, sum / f64::from(count)))
            .collect()
    }
}

/// How a turn ended, for what it says of a whole turn's cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnEnd {
    /// It got to its end — or to an `await`, which is what the script asked
    /// for: it completes what it is priced at.
    Finished {
        /// Whether it ran to its end, rather than to an `await`.
        completed: bool,
    },
    /// Cut by the fuel, or its `Update` deferred: a piece of a turn.
    Piece,
    /// It faulted: what it had spent prices nothing.
    Faulted,
}

impl TurnEnd {
    /// How `outcome` ended its turn.
    pub(super) fn of(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Completed { .. } => Self::Finished { completed: true },
            Outcome::Busy { starved: false, .. } => Self::Finished { completed: false },
            Outcome::Faulted { .. } => Self::Faulted,
            _ => Self::Piece,
        }
    }
}

/// Records what `held`'s turn cost toward its price: the pieces of a turn cut
/// across frames add up until it finishes, and then they are its whole cost.
pub(super) fn price_turn(held: &mut Instance, end: TurnEnd, cost: u64) {
    match end {
        TurnEnd::Faulted => held.piece_spent = 0,
        TurnEnd::Finished { completed } => {
            let whole = held.piece_spent + cost;
            held.piece_spent = 0;
            // A turn that only waited ran nothing: it says nothing of what a
            // turn costs. One that completed having nothing to do did: it
            // costs nothing.
            if whole > 0 || completed {
                held.full_turn = Some(whole);
            }
        }
        TurnEnd::Piece => held.piece_spent += cost,
    }
}

/// Whether this frame advanced any of the behavior's countdowns.
///
/// Its own question because a countdown is the one part of an instance that
/// changes without the behavior running: `count_down` decrements what is armed
/// whether or not anything comes due, so fuel spent is not the whole test of
/// "did this instance change".
pub(super) fn delta_moved_a_countdown(
    program: &khora_script::vm::Program,
    behavior: &str,
    delta: f32,
) -> bool {
    delta > 0.0
        && program
            .layout(behavior)
            .is_some_and(|layout| !layout.timers.is_empty())
}
