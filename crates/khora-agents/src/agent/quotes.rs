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

//! When a family's rows are priced: the running row by its own run, the
//! other lanes by quoting them on their inputs when the quotes may no longer
//! hold.

use std::time::{Duration, Instant};

use khora_core::lane::io::{Target, TargetKey};
use khora_core::renderer::api::frame::FrameContext;
use khora_core::EngineContext;

use super::{lend, time_of, LaneAgent};

/// How far the running row's quote may move from its value at the last full
/// quote, as a fraction of that value, before every row is quoted again.
pub const QUOTE_DRIFT: f32 = 0.10;

/// How long a full quote stands before every row is quoted again, even when
/// nothing drifted.
pub const QUOTE_MAX_AGE: Duration = Duration::from_secs(1);

/// The last time every row of the menu was quoted.
#[derive(Debug, Clone)]
pub(super) struct FullQuote {
    /// When.
    at: Instant,
    /// The frame's target key then; `None` when no lane reads the frame.
    key: Option<TargetKey>,
    /// Every row's quote then, by row: the base each row's drift is measured
    /// from when it is the one running. `None` for a row left unquoted.
    bases: Vec<Option<Duration>>,
}

/// Whether `now` moved from `base` by more than [`QUOTE_DRIFT`] of it.
fn drifted(base: Duration, now: Duration) -> bool {
    if base.is_zero() {
        return !now.is_zero();
    }
    let change = (now.as_secs_f64() - base.as_secs_f64()).abs();
    change > base.as_secs_f64() * f64::from(QUOTE_DRIFT)
}

impl LaneAgent {
    /// Quotes every lane but the one that just ran (its run quoted it) when
    /// the quotes may no longer hold: the first time, when the frame's target
    /// changed, when the running row's quote drifted past [`QUOTE_DRIFT`]
    /// from its value at the last full quote, or when that quote is older
    /// than [`QUOTE_MAX_AGE`]. Otherwise the other lanes keep their quotes.
    ///
    /// The agent reads no world state to decide: a change in the world
    /// reaches it only through the Views its lanes read, as a drift of the
    /// running row's quote.
    pub(super) fn requote(
        &mut self,
        ran: usize,
        context: &EngineContext<'_>,
        frame: Option<&FrameContext>,
    ) {
        let ran_lane = self.menu[ran].lane;
        // A run that priced nothing (refused for want of a budget it was never
        // granted) is quoted on its inputs, so GORNA has a price to grant.
        if self.quotes.get(ran).copied().flatten().is_none() {
            self.quote_lane(ran_lane, context, frame);
        }
        let Some(running) = self.quotes.get(ran).copied().flatten().map(|c| time_of(&c)) else {
            return;
        };
        let key = frame
            .and_then(|frame| frame.get::<Target>())
            .map(|target| target.key);
        let due = match &self.full_quote {
            None => true,
            Some(last) => {
                last.key != key
                    || last.at.elapsed() > QUOTE_MAX_AGE
                    // The row that ran, against its own quote at the last full
                    // quote: a row GORNA moved to since is measured from its
                    // price then, and one unquoted then counts as moved.
                    || last
                        .bases
                        .get(ran)
                        .copied()
                        .flatten()
                        .is_none_or(|base| drifted(base, running))
            }
        };
        if !due {
            return;
        }
        for lane in (0..self.lanes.len()).filter(|&lane| lane != ran_lane) {
            self.quote_lane(lane, context, frame);
        }
        self.full_quote = Some(FullQuote {
            at: Instant::now(),
            key,
            bases: self
                .quotes
                .iter()
                .map(|quote| quote.as_ref().map(time_of))
                .collect(),
        });
    }

    /// Quotes every level of `lane` on this frame's inputs, lent the time the
    /// agent was granted: a budgeted lane prices its own budget. Its rows are
    /// left unquoted when it cannot be.
    fn quote_lane(
        &mut self,
        lane: usize,
        context: &EngineContext<'_>,
        frame: Option<&FrameContext>,
    ) {
        let budget = self.time_limit;
        let quoted = {
            let mut source = lend(context, budget, self.fixed_step, frame);
            self.lanes[lane]
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .quote(&mut source)
        };
        for (row, offered) in self.menu.iter().zip(self.quotes.iter_mut()) {
            if row.lane == lane {
                *offered = quoted
                    .as_ref()
                    .and_then(|costs| costs.get(row.level).copied());
            }
        }
    }
}
