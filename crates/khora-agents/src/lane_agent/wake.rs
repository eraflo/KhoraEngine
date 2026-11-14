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

//! How a family's agent answers its wakes: what the last run found, the
//! fallback from a row that failed, the quotes, the choice, and the layer it
//! installs at its place.

use std::any::TypeId;
use std::sync::{Arc, MutexGuard};
use std::time::Instant;

use khora_core::agent::contention::Contention;
use khora_core::agent::gorna::protocol::{Epoch, Report};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::{Agent, Layer, Place, Preparation, Terms, Wake};
use khora_core::lane::erased::NotPrepared;
use khora_core::lane::guard::Reason;
use khora_core::renderer::api::frame::FrameContext;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::EngineContext;

use super::layer::{gpu_with, LaneLayer};
use super::{lend, time_of, LaneAgent, Row};

/// Whether a refusal for `reason` leaves the lane without a price: an input
/// absent or unusable. A lock not obtained in time is contention, and a
/// missing budget GORNA's own doing: the price stands.
fn unusable(reason: Reason) -> bool {
    matches!(
        reason,
        Reason::Missing
            | Reason::Stale
            | Reason::Undeclared
            | Reason::BadTarget
            | Reason::LockPoisoned
    )
}

impl LaneAgent {
    /// The row chosen, if the menu has any.
    pub(super) fn row(&self) -> Option<Row> {
        self.menu.get(self.current).copied()
    }

    /// The row of `id`, as an index into the menu: a family offers each id
    /// once.
    pub(super) fn row_of(&self, id: StrategyId) -> Option<usize> {
        self.menu.iter().position(|row| row.id == id)
    }

    /// The agent's place, locked.
    pub(super) fn locked_place(&self) -> MutexGuard<'_, Place> {
        self.place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The row running: the one installed at `place`, else the one chosen.
    pub(super) fn running(&self, place: &Place) -> StrategyId {
        place
            .installed_row()
            .or_else(|| self.row().map(|row| row.id))
            .unwrap_or(StrategyId::Balanced)
    }

    /// The row that ran last at `place`, if any did.
    pub(super) fn ran(&self, place: &Place) -> Option<usize> {
        let report = place.last_run().report.as_ref()?;
        self.row_of(report.row)
    }

    /// Takes in the costs the last run at `place` found: the price of every
    /// level of the lane that ran.
    pub(super) fn absorb_last_run(&mut self, place: &Place) {
        let Some(ran) = self.ran(place) else {
            return;
        };
        let lane = self.menu[ran].lane;
        let first_of_lane = self
            .menu
            .iter()
            .position(|candidate| candidate.lane == lane)
            .unwrap_or(0);
        for (level, cost) in place.last_run().costs.iter().enumerate() {
            if let Some(quote) = self.quotes.get_mut(first_of_lane + level) {
                *quote = Some(*cost);
            }
        }
    }

    /// The frame's target and clear colour, for a family one of whose lanes
    /// reads them: only such a contention names the `FrameContext`.
    pub(super) fn frame(&self, context: &EngineContext<'_>) -> Option<Arc<FrameContext>> {
        let frame_context = TypeId::of::<Arc<FrameContext>>();
        self.contentions
            .iter()
            .any(|contention| contention.reads.contains(&frame_context))
            .then(|| context.resource::<Arc<FrameContext>>().cloned())
            .flatten()
    }

    /// Marks `row` failed under the envelope lent, so the choice passes it by
    /// until the next allocation — its own lane's runs would price it again,
    /// and it would fail again — and falls back from it: within the envelope
    /// once one is lent, to a cheaper row before the first allocation.
    pub(super) fn fail(&mut self, row: usize) {
        if self.lent.is_some() {
            if let Some(failed) = self.failed.get_mut(row) {
                *failed = true;
            }
        }
        self.current = self.cheaper_row(row);
        self.choose_row();
    }

    /// Installs the row chosen at `place`: nothing when it is installed or
    /// staged already; the layer kept for it, reinstated, preparing nothing;
    /// beside an installed layer, a preparation of the new one, left at the
    /// place for the scheduler to run off the frame while the installed one
    /// keeps running; or, into an empty place, a new layer prepared here on
    /// this frame's inputs and installed at once. A row that fails to prepare
    /// here is failed, and the row the agent falls back to is installed
    /// instead.
    pub(super) fn install(
        &mut self,
        place: &mut Place,
        context: &EngineContext<'_>,
        frame: Option<&Arc<FrameContext>>,
    ) {
        let device = context.resource::<Arc<dyn GraphicsDevice>>().cloned();
        let pipelines = context.resource::<Arc<dyn PipelineSystem>>().cloned();
        let mut gpu = gpu_with(device.as_deref(), pipelines.as_deref());
        for _ in 0..=self.menu.len() {
            let Some(row) = self.row() else {
                return;
            };
            if place.reinstate(row.id) {
                return;
            }
            let lane_frame = self
                .contentions
                .get(row.lane)
                .filter(|contention| {
                    contention
                        .reads
                        .contains(&TypeId::of::<Arc<FrameContext>>())
                })
                .and(frame);
            if place.installed_row().is_some() {
                let step = place.fixed_step();
                place.request_preparation(self.preparation(row, lane_frame.cloned(), step));
                return;
            }
            let lane_frame = lane_frame.map(|frame| &**frame);
            let mut lane = (self.makers[row.lane])();
            let started = Instant::now();
            let prepared = {
                let mut source = lend(context, self.budget(), place.fixed_step(), lane_frame);
                lane.prepare(row.level, &mut source, &mut gpu)
            };
            let switch = started.elapsed();
            match prepared {
                // A refused input leaves the target unknown: the first run
                // prepares the layer.
                Ok(()) | Err(NotPrepared::Refused(_)) => {
                    let layer = LaneLayer::new(
                        lane,
                        row.level,
                        row.id,
                        self.registration.family_name,
                        row.memory,
                    );
                    place.stage(Box::new(layer), switch);
                    return;
                }
                Err(NotPrepared::Failed(error)) => {
                    log::warn!(
                        "{}: `{}` could not be prepared at row {:?}: {error}",
                        self.registration.family_name,
                        lane.name(),
                        row.id
                    );
                    lane.shutdown(&mut gpu);
                    let failed = self.current;
                    self.fail(failed);
                    if self.current == failed {
                        // Nothing to fall back to: the row runs, and fails
                        // where it can be seen.
                        let layer = LaneLayer::new(
                            (self.makers[row.lane])(),
                            row.level,
                            row.id,
                            self.registration.family_name,
                            row.memory,
                        );
                        place.stage(Box::new(layer), switch);
                        return;
                    }
                }
            }
        }
    }

    /// The preparation of `row`'s layer, off the frame: a fresh lane, made
    /// here, prepared by the job on the context the scheduler lends it — the
    /// inputs lent as for a run, on `frame` when the lane reads it — under
    /// the lane's own declaration and the graphics it prepares with. A lane
    /// that fails to prepare is shut down, and the job says why.
    fn preparation(
        &self,
        row: Row,
        frame: Option<Arc<FrameContext>>,
        fixed_step: Option<std::time::Duration>,
    ) -> Preparation {
        let mut lane = (self.makers[row.lane])();
        let contention = self
            .contentions
            .get(row.lane)
            .cloned()
            .unwrap_or_else(Contention::none)
            .reading::<Arc<dyn GraphicsDevice>>()
            .reading::<Arc<dyn PipelineSystem>>();
        let budget = self.budget();
        let family = self.registration.family_name;
        Preparation::new(row.id, contention, move |context| {
            let device = context.resource::<Arc<dyn GraphicsDevice>>().cloned();
            let pipelines = context.resource::<Arc<dyn PipelineSystem>>().cloned();
            let mut gpu = gpu_with(device.as_deref(), pipelines.as_deref());
            let prepared = {
                let mut source = lend(context, budget, fixed_step, frame.as_deref());
                lane.prepare(row.level, &mut source, &mut gpu)
            };
            match prepared {
                // A refused input leaves the target unknown: the first run
                // prepares the layer.
                Ok(()) | Err(NotPrepared::Refused(_)) => {
                    let layer: Box<dyn Layer> =
                        Box::new(LaneLayer::new(lane, row.level, row.id, family, row.memory));
                    Ok(layer)
                }
                Err(NotPrepared::Failed(error)) => {
                    let name = lane.name().to_owned();
                    lane.shutdown(&mut gpu);
                    Err(format!(
                        "{family}: `{name}` could not be prepared at row {:?}: {error}",
                        row.id
                    ))
                }
            }
        })
    }

    /// Answers a preparation that failed off the frame for `id`'s row: the
    /// row is failed, as one failing to prepare here is. Returns the row the
    /// agent is left on when there is nothing to fall back to.
    fn prepare_failed(&mut self, id: StrategyId) -> Option<usize> {
        let failed = self.menu.iter().position(|row| row.id == id)?;
        self.fail(failed);
        (self.current == failed).then_some(failed)
    }

    /// Stages `row`'s layer unprepared: what is left when it failed to
    /// prepare and there is nothing to fall back to — it runs, and fails
    /// where it can be seen.
    fn stage_unprepared(&self, row: usize, place: &mut Place) {
        let row = self.menu[row];
        let layer = LaneLayer::new(
            (self.makers[row.lane])(),
            row.level,
            row.id,
            self.registration.family_name,
            row.memory,
        );
        place.stage(Box::new(layer), std::time::Duration::ZERO);
    }

    /// The terms the chosen row runs on, at the `fixed_step` its place holds.
    pub(super) fn terms(&self, fixed_step: Option<std::time::Duration>) -> Terms {
        let full = self.full_quote.as_ref();
        Terms {
            epoch: self
                .lent
                .as_ref()
                .map_or(Epoch::default(), |lent| lent.epoch),
            envelope: self.lent.as_ref().and_then(|lent| lent.envelope),
            budget: self.budget(),
            fixed_step,
            over_budget: self.over_budget,
            quote: full.and_then(|full| full.bases.get(self.current).copied().flatten()),
            quoted_at: full.map(|full| full.at),
            target: full.and_then(|full| full.key),
        }
    }

    /// The row to fall back to from `from`: the dearest row quoted cheaper
    /// than `from`, or, with no quote to go by, the level below in the same
    /// lane — a lane's levels come cheapest first, a family's lanes in no
    /// order. `from` itself when nothing is known to be cheaper.
    pub(super) fn cheaper_row(&self, from: usize) -> usize {
        let quoted = |row: usize| self.quotes.get(row).copied().flatten().map(|c| time_of(&c));
        if let Some(limit) = quoted(from) {
            let cheaper = (0..self.menu.len())
                .filter_map(|row| quoted(row).map(|time| (row, time)))
                .filter(|&(_, time)| time < limit)
                .max_by_key(|&(_, time)| time);
            if let Some((row, _)) = cheaper {
                return row;
            }
        }
        match (from.checked_sub(1), self.menu.get(from)) {
            (Some(below), Some(current)) if self.menu[below].lane == current.lane => below,
            _ => from,
        }
    }

    /// One frame of the family's place for a caller that drives the agent
    /// itself: the wakes waiting answered, the frame start, the installed
    /// layer's run, and the wakes that run raised answered at once, so the
    /// next call runs what they chose.
    pub(super) fn whole_frame(&mut self, context: &mut EngineContext<'_>) -> Option<Report> {
        let place = Arc::clone(&self.place);
        let mut place = place
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let why = place.take_wakes();
        if !why.is_empty() {
            self.wake(&why, &mut place, context);
        }
        self.prepare_here(&mut place, context);
        {
            let device = context.resource::<Arc<dyn GraphicsDevice>>().cloned();
            let pipelines = context.resource::<Arc<dyn PipelineSystem>>().cloned();
            place.start_frame(&mut gpu_with(device.as_deref(), pipelines.as_deref()));
        }
        let report = place.run(context);
        let why = place.take_wakes();
        if !why.is_empty() {
            self.wake(&why, &mut place, context);
        }
        self.prepare_here(&mut place, context);
        report
    }

    /// Runs here, at once, the preparation the agent left at `place`, for a
    /// caller with no scheduler to run it off the frame; one that fails is
    /// answered at once too, so the row fallen back to is prepared as well.
    fn prepare_here(&mut self, place: &mut Place, context: &mut EngineContext<'_>) {
        for _ in 0..=self.menu.len() {
            let Some(preparation) = place.take_preparation() else {
                return;
            };
            place.prepared(preparation.run(context));
            let why = place.take_wakes();
            if why.is_empty() {
                return;
            }
            self.wake(&why, place, context);
        }
    }
    /// Takes in what the last run found, answers each wake, quotes every lane
    /// when the quotes may no longer hold, chooses the row within the
    /// envelope, installs it, and sets the terms it runs on.
    pub(super) fn answer(
        &mut self,
        why: &[Wake],
        place: &mut Place,
        context: &mut EngineContext<'_>,
    ) {
        self.absorb_last_run(place);
        let ran = self.ran(place);
        let mut full = false;
        // A row that failed to prepare off the frame, with nothing to fall
        // back to.
        let mut stranded = None;
        for wake in why {
            match wake {
                Wake::FirstFrame | Wake::QuoteDrift | Wake::QuoteAge | Wake::TargetChanged => {
                    full = true;
                }
                Wake::LaneFailed => {
                    if let Some(ran) = ran {
                        self.fail(ran);
                    }
                }
                Wake::InputRefused(reason) => {
                    // A lane refused an input that is absent or unusable has
                    // no price: its rows are not chosen on the price they had
                    // before.
                    if let (true, Some(ran)) = (unusable(*reason), ran) {
                        let lane = self.menu[ran].lane;
                        for (candidate, quote) in self.menu.iter().zip(self.quotes.iter_mut()) {
                            if candidate.lane == lane {
                                *quote = None;
                            }
                        }
                    }
                }
                Wake::Allocated | Wake::QuoteUnfit => {}
                Wake::PrepareFailed(row) => {
                    stranded = self.prepare_failed(*row);
                }
            }
        }
        let frame = self.frame(context);
        if full {
            // Every lane quoted on the inputs as they stand, the one that ran
            // too: its run priced what it was given, which a lane that takes
            // its requests no longer holds, and rows priced on other inputs
            // would be compared unfairly.
            self.quote_every_lane(context, frame.as_deref(), place.fixed_step());
        } else if let Some(ran) = ran.filter(|ran| self.quotes[*ran].is_none()) {
            // A run that priced nothing (refused for want of a budget it was
            // never granted) is quoted on its inputs, so GORNA has a price
            // to grant.
            let lane = self.menu[ran].lane;
            self.quote_lane(lane, context, frame.as_deref(), place.fixed_step());
        }
        self.choose_row();
        match stranded.filter(|&row| row == self.current) {
            Some(row) => self.stage_unprepared(row, place),
            None => self.install(place, context, frame.as_ref()),
        }
        place.set_terms(self.terms(place.fixed_step()));
    }
}
