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

//! Evaluating the arbiter, with no I/O: a recorded journal decided again by
//! a set of proposers ([`replay`]), and a simulated engine run through
//! scenarios ([`simulate`]).

mod replay;
mod scenarios;
mod simulation;
#[cfg(test)]
mod tests;

use crate::gorna::arbiter::{Arbiter, Greedy};

pub use replay::{replay, Replay, ReplayMetrics, ReplayedEpoch};
pub use scenarios::{simulate, Scenario};
pub use simulation::{SimAgent, SimDecision, SimFrame, SimMetrics, Simulation};

/// The proposer sets compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arbiters {
    /// The Q-RAM pass alone.
    Greedy,
    /// The Q-RAM pass, then the exact search started from its plan, deciding
    /// without planning beyond the frame.
    GreedyExact,
    /// The Q-RAM pass then the exact search, on the planned problem: the
    /// switch charges, the margin and the growth, each row's host time
    /// corrected by the cost book. The engine's default.
    Planned,
    /// The planned set on the agents' raw quotes: no cost book.
    PlannedOnQuotes,
}

impl Arbiters {
    /// Every proposer set compared.
    pub const ALL: [Arbiters; 4] = [
        Arbiters::Greedy,
        Arbiters::GreedyExact,
        Arbiters::Planned,
        Arbiters::PlannedOnQuotes,
    ];

    /// The set's name, as a table prints it.
    pub fn name(self) -> &'static str {
        match self {
            Arbiters::Greedy => "greedy",
            Arbiters::GreedyExact => "greedy+exact",
            Arbiters::Planned => "planned",
            Arbiters::PlannedOnQuotes => "planned on quotes",
        }
    }

    /// The arbiter running this set's proposers.
    pub(crate) fn arbiter(self) -> Arbiter {
        match self {
            Arbiters::Greedy => Arbiter::with(vec![Box::new(Greedy)]),
            Arbiters::GreedyExact | Arbiters::Planned => Arbiter::default(),
            Arbiters::PlannedOnQuotes => Arbiter::default(),
        }
    }
}
