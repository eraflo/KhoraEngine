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

//! A family's place in the frame, and the frame logic every layered agent
//! shares: the swap at frame start, the kept layers, the wakes, the run and
//! its report. The scheduler and the agents' tests run this same code.

use std::time::{Duration, Instant};

use super::{Layer, LayerRun, Wake};
use crate::agent::contention::Contention;
use crate::agent::gorna::protocol::{Envelope, Epoch, Report};
use crate::agent::gorna::{AgentId, StrategyId};
use crate::gpu::GpuSetup;
use crate::lane::cost::{Cost, PerDevice};
use crate::lane::io::TargetKey;
use crate::EngineContext;

/// How many epochs a layer swapped out is kept ready: each allocation lent
/// counts one down, and a layer with none left is shut down at the next frame
/// start.
pub const KEEP_EPOCHS: u32 = 3;

/// What the agent lent its installed layer: the terms every run is made on.
///
/// Two parts. The agent's — `epoch`, `envelope`, `time_limit`,
/// `budget_lent`, `fixed_step`, `over_budget`, `quoted_at`, `target` — is
/// what the DCC lent the family and how the agent answered it, from its last
/// full quote, whatever layer runs it: set while another layer is on its way
/// in, it applies to the installed one at once, so its reports answer the
/// epoch lent. The row's — its `quote`, its own price — applies from that
/// row's swap.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Terms {
    /// The epoch the agent runs in.
    pub epoch: Epoch,
    /// The envelope the DCC lent; `None` while none was, which limits
    /// nothing — the kept layers' memory included.
    pub envelope: Option<Envelope>,
    /// The time lent to a `#[budget]` lane; zero lends none, unless
    /// `budget_lent` says a budget of zero was lent.
    pub time_limit: Duration,
    /// Whether the agent was lent a budget at all: a lane taking
    /// `Option<Budget>` reads `None` until it is, and `time_limit` once it
    /// is — zero included.
    pub budget_lent: bool,
    /// The simulated time one run advances, for a fixed-step family.
    pub fixed_step: Option<Duration>,
    /// Whether the row is the floor, run because no row fitted the envelope.
    pub over_budget: bool,
    /// The row's quote when the agent last quoted every row: what its drift
    /// is measured from — its time, and the impact it named. `None` when it
    /// was not quoted then; the layer's first run then sets the base.
    pub quote: Option<Cost>,
    /// When the agent last quoted every row; `None` before it ever did.
    pub quoted_at: Option<Instant>,
    /// The frame's target key then; `None` when the family reads no target.
    pub target: Option<TargetKey>,
}

impl Terms {
    /// `lent` with the row's part of these terms: their quote, the row's
    /// own price, kept.
    fn lending(&self, lent: &Terms) -> Terms {
        Terms {
            quote: self.quote,
            ..lent.clone()
        }
    }
}

/// A layer at the place, with the time its preparation took while its first
/// run has not reported it yet.
struct Held {
    layer: Box<dyn Layer>,
    switch: Option<Duration>,
}

/// A layer swapped out and kept ready, with the epochs it has left.
struct Kept {
    layer: Box<dyn Layer>,
    epochs: u32,
}

/// What the last run of a place gave back, for an agent reading its place
/// between two wakes.
#[derive(Debug, Clone, Default)]
pub struct LastRun {
    /// The report the run published; `None` before any run.
    pub report: Option<Report>,
    /// The cost of every level of the layer's lane on that run's inputs, in
    /// level order; empty when the inputs were refused.
    pub costs: Vec<Cost>,
    /// What went wrong on that run; empty when nothing did.
    pub message: String,
}

/// A family's place in the frame: its phase and its pass slot, where one
/// [`Layer`] is installed.
///
/// It holds the installed layer, the layer staged for the next frame start,
/// the layers swapped out and kept ready (each with the epochs it has left),
/// the wakes gathered for the agent, the [`Terms`] the agent lent, and the
/// time the staged layer took to prepare, which the first report after its
/// swap carries.
///
/// Shared between the agent and the scheduler as `Arc<Mutex<Place>>`. Lock
/// order: an agent's lock, then its place's — never the other way round.
pub struct Place {
    agent: AgentId,
    installed: Option<Held>,
    staged: Option<Held>,
    /// Oldest first.
    kept: Vec<Kept>,
    wakes: Vec<Wake>,
    terms: Terms,
    /// The terms set while a layer was staged: they are the staged layer's,
    /// and apply from its swap.
    staged_terms: Option<Terms>,
    last: LastRun,
    /// The row of the installed layer while it is checked out for a run off
    /// the frame; `None` while it is home.
    out: Option<StrategyId>,
    /// The preparation left for a runner and not taken yet.
    waiting: Option<super::Preparation>,
    /// The row of the preparation a runner took and has not handed back.
    under_way: Option<StrategyId>,
    /// The row the agent last asked a preparation for, while it still wants
    /// it: what a preparation's layer is staged for. A stage or a reinstate
    /// since means the agent wants another layer.
    wanted: Option<StrategyId>,
}

/// The installed layer of a place, checked out for one run off the frame,
/// with the terms it runs on. The place holds nothing of it meanwhile, so no
/// lock is held across frames; it comes back through [`Place::check_in`].
pub struct CheckedOut {
    held: Held,
    terms: Terms,
}

impl CheckedOut {
    /// The row the layer runs.
    pub fn row(&self) -> StrategyId {
        self.held.layer.row()
    }

    /// One run of the layer, on the terms it was checked out with.
    pub fn run(&mut self, context: &mut EngineContext<'_>) -> LayerRun {
        self.held.layer.run(&self.terms, context)
    }

    /// What the layer competes for: the permit its run is stamped with.
    pub fn contention(&self) -> Contention {
        self.held.layer.contention()
    }

    /// What the run staged, for the reserve.
    pub fn staged(&mut self) -> crate::lane::Staging {
        self.held.layer.staged()
    }
}

impl Place {
    /// The place of `agent`'s family: nothing installed, and a
    /// [`Wake::FirstFrame`] waiting, so the first event pass wakes the agent
    /// to install a layer before the first run.
    pub fn new(agent: AgentId) -> Self {
        Self {
            agent,
            installed: None,
            staged: None,
            kept: Vec::new(),
            wakes: vec![Wake::FirstFrame],
            terms: Terms::default(),
            staged_terms: None,
            last: LastRun::default(),
            out: None,
            waiting: None,
            under_way: None,
            wanted: None,
        }
    }

    /// Checks the installed layer out for one run off the frame: `None` while
    /// nothing is installed, or it is out already. Until it is checked in, the
    /// place swaps in no staged layer and runs nothing.
    pub fn check_out(&mut self) -> Option<CheckedOut> {
        if self.out.is_some() {
            return None;
        }
        let held = self.installed.take()?;
        self.out = Some(held.layer.row());
        Some(CheckedOut {
            held,
            terms: self.terms.clone(),
        })
    }

    /// Takes back the layer `out` held, with what its `run` gave back, and
    /// reports the run as [`run`](Self::run) does. A layer staged while it was
    /// out waits for the next frame start.
    pub fn check_in(&mut self, out: CheckedOut, run: LayerRun) -> Report {
        let CheckedOut { mut held, terms } = out;
        self.out = None;
        let row = held.layer.row();
        let switch = held.switch.take();
        self.installed = Some(held);
        self.report(row, &terms, switch, run)
    }

    /// Whether the installed layer is checked out for a run off the frame.
    pub fn is_out(&self) -> bool {
        self.out.is_some()
    }

    /// The row of the layer installed, if one is.
    pub fn installed_row(&self) -> Option<StrategyId> {
        self.installed
            .as_ref()
            .map(|held| held.layer.row())
            .or(self.out)
    }

    /// The row of the layer staged for the next frame start, if one is.
    pub fn staged_row(&self) -> Option<StrategyId> {
        self.staged.as_ref().map(|held| held.layer.row())
    }

    /// The rows of the layers kept ready, oldest first, each with the epochs
    /// it has left.
    pub fn kept(&self) -> Vec<(StrategyId, u32)> {
        self.kept
            .iter()
            .map(|kept| (kept.layer.row(), kept.epochs))
            .collect()
    }

    /// The wakes gathered since the agent was last woken, oldest first.
    pub fn wakes(&self) -> &[Wake] {
        &self.wakes
    }

    /// Adds `why` to the wakes the agent is woken for at the next event pass.
    pub fn raise(&mut self, why: Wake) {
        self.wakes.push(why);
    }

    /// Takes the wakes gathered, leaving none: what the event pass hands the
    /// agent.
    pub fn take_wakes(&mut self) -> Vec<Wake> {
        std::mem::take(&mut self.wakes)
    }

    /// The terms the installed layer runs on.
    pub fn terms(&self) -> &Terms {
        &self.terms
    }

    /// Sets the terms the installed layer runs on, from its next run. While
    /// a layer is staged, or prepared, they are that layer's, whole from its
    /// swap; the installed layer runs on their agent's part at once and keeps
    /// its own row's part until then.
    pub fn set_terms(&mut self, terms: Terms) {
        if self.staged.is_some() || self.pending_row().is_some() {
            self.terms = self.terms.lending(&terms);
            self.staged_terms = Some(terms);
        } else {
            self.terms = terms;
        }
    }

    /// Sets the step every layer of the place runs at — the installed one
    /// and one on its way in — leaving the rest of their terms as they are.
    pub fn set_fixed_step(&mut self, step: Option<Duration>) {
        self.terms.fixed_step = step;
        if let Some(staged) = self.staged_terms.as_mut() {
            staged.fixed_step = step;
        }
    }

    /// What the last run gave back: its report, the costs it found and what
    /// went wrong.
    pub fn last_run(&self) -> &LastRun {
        &self.last
    }

    /// Stages `layer`, prepared in `switch`, to be swapped in at the next
    /// frame start. Into a place with nothing installed it is installed at
    /// once: no frame runs without a layer. A layer staged before it and
    /// never swapped in is kept ready. A preparation asked for before is no
    /// longer wanted: one waiting is dropped, one under way kept once it ends.
    pub fn stage(&mut self, layer: Box<dyn Layer>, switch: Duration) {
        self.unwant();
        self.stage_held(layer, switch);
    }

    fn stage_held(&mut self, layer: Box<dyn Layer>, switch: Duration) {
        let mut held = Held {
            layer,
            switch: Some(switch),
        };
        if self.installed.is_none() && self.out.is_none() {
            held.layer.installed();
            self.installed = Some(held);
            return;
        }
        if let Some(previous) = self.staged.replace(held) {
            self.keep(previous.layer);
        }
    }

    /// Stages the kept layer of `row` again, preparing nothing; whether one
    /// was kept. The installed layer's own row unstages what was staged,
    /// keeping it ready; the staged layer's own row changes nothing. A
    /// preparation asked for before is no longer wanted, once one is found.
    pub fn reinstate(&mut self, row: StrategyId) -> bool {
        let found = self.installed_row() == Some(row)
            || self.staged_row() == Some(row)
            || self.kept.iter().any(|kept| kept.layer.row() == row);
        if !found {
            return false;
        }
        self.unwant();
        if self.installed_row() == Some(row) {
            if let Some(staged) = self.staged.take() {
                self.keep(staged.layer);
            }
            // The row's part was the staged layer's; the agent's part
            // applies already.
            self.staged_terms = None;
            return true;
        }
        if self.staged_row() == Some(row) {
            return true;
        }
        let Some(index) = self.kept.iter().rposition(|kept| kept.layer.row() == row) else {
            return false;
        };
        let kept = self.kept.remove(index);
        self.stage_held(kept.layer, Duration::ZERO);
        true
    }

    /// Forgets the preparation asked for: one waiting is dropped, and the
    /// layer of one under way will be kept rather than staged.
    fn unwant(&mut self) {
        self.waiting = None;
        self.wanted = None;
    }

    /// Leaves `preparation` at the place, to be taken by whoever runs it
    /// ([`take_preparation`](Self::take_preparation)); the installed layer
    /// runs on meanwhile. Its row is the one wanted from now: a preparation
    /// waiting and not yet taken is replaced by it, and the layer of one
    /// under way is kept ready once it ends.
    pub fn request_preparation(&mut self, preparation: super::Preparation) {
        // A layer staged before is not the one wanted any more.
        if let Some(staged) = self.staged.take() {
            self.keep(staged.layer);
        }
        self.wanted = Some(preparation.row());
        self.waiting = Some(preparation);
    }

    /// Takes the preparation waiting at the place, for its runner; `None`
    /// while none waits.
    pub fn take_preparation(&mut self) -> Option<super::Preparation> {
        let preparation = self.waiting.take()?;
        self.under_way = Some(preparation.row());
        Some(preparation)
    }

    /// The row of the preparation the place waits for — waiting to be taken
    /// or under way — if it waits for one.
    pub fn pending_row(&self) -> Option<StrategyId> {
        let pending = self
            .waiting
            .as_ref()
            .map(super::Preparation::row)
            .or(self.under_way);
        self.wanted.filter(|_| pending.is_some())
    }

    /// Takes back what a preparation gave. A layer whose row is still the
    /// one wanted is staged, with the time its preparation took; one whose
    /// row no longer is is kept ready. A preparation that gave no layer
    /// raises [`Wake::PrepareFailed`] for its row.
    pub fn prepared(&mut self, prepared: super::Prepared) {
        let row = prepared.row();
        if self.under_way == Some(row) {
            self.under_way = None;
        }
        let wanted = self.wanted == Some(row);
        match prepared.into_outcome() {
            Ok((layer, took)) if wanted => {
                self.wanted = None;
                if self.waiting.as_ref().map(super::Preparation::row) == Some(row) {
                    self.waiting = None;
                }
                self.stage_held(layer, took);
            }
            Ok((layer, _)) => self.keep(layer),
            Err(message) => {
                log::warn!(
                    "the layer of row {row:?} of `{}` could not be prepared: {message}",
                    self.agent
                );
                if wanted {
                    self.wanted = None;
                    // The terms were the failed layer's: the agent sets new
                    // ones when it chooses again.
                    if self.staged.is_none() {
                        self.staged_terms = None;
                    }
                    self.raise(Wake::PrepareFailed(row));
                }
            }
        }
    }

    /// Records that an allocation for `epoch` was lent to the agent: the
    /// agent is woken for it ([`Wake::Allocated`]) and every kept layer has
    /// one epoch less.
    pub fn lent(&mut self, epoch: Epoch) {
        let _ = epoch;
        self.raise(Wake::Allocated);
        for kept in &mut self.kept {
            kept.epochs = kept.epochs.saturating_sub(1);
        }
    }

    /// The frame start: swaps the staged layer in and keeps the one it
    /// replaces for [`KEEP_EPOCHS`] epochs; then shuts down, with `gpu`, every
    /// kept layer with no epoch left and, oldest first, those the envelope's
    /// memory cannot hold beside the installed one.
    pub fn start_frame(&mut self, gpu: &mut GpuSetup<'_>) {
        // A layer out for a run off the frame is swapped once it is back.
        if self.out.is_none() {
            self.swap_staged();
        }
        self.shut_down_spent(gpu);
    }

    fn swap_staged(&mut self) {
        if let Some(mut staged) = self.staged.take() {
            staged.layer.installed();
            if let Some(old) = self.installed.replace(staged) {
                self.keep(old.layer);
            }
            if let Some(terms) = self.staged_terms.take() {
                self.terms = terms;
            }
        }
    }

    fn shut_down_spent(&mut self, gpu: &mut GpuSetup<'_>) {
        let mut index = 0;
        while index < self.kept.len() {
            if self.kept[index].epochs == 0 {
                self.kept.remove(index).layer.shutdown(gpu);
            } else {
                index += 1;
            }
        }
        if let Some(envelope) = self.terms.envelope {
            while !self.kept.is_empty() && !self.memory_fits(&envelope) {
                self.kept.remove(0).layer.shutdown(gpu);
            }
        }
    }

    /// Runs the installed layer on the terms, keeps the wake it raised, and
    /// reports the run: the terms' epoch and over-budget flag, the layer's
    /// row, what it measured, did and stalled on, a health of the lesser of
    /// its time ratio and what it did, and — on the first run after a swap —
    /// the time its preparation took. `None`, running nothing, while nothing
    /// is installed.
    pub fn run(&mut self, context: &mut EngineContext<'_>) -> Option<Report> {
        let held = self.installed.as_mut()?;
        let run = held.layer.run(&self.terms, context);
        let row = held.layer.row();
        let switch = held.switch.take();
        let terms = self.terms.clone();
        Some(self.report(row, &terms, switch, run))
    }

    /// Keeps the wake `run` raised and what it gave back, and reports it on
    /// `terms`.
    fn report(
        &mut self,
        row: StrategyId,
        terms: &Terms,
        switch: Option<Duration>,
        run: LayerRun,
    ) -> Report {
        if let Some(why) = run.wake {
            self.raise(why);
        }
        // A share out of range is held to it; one that is not a number says
        // nothing was done.
        let done = if run.done.is_nan() {
            0.0
        } else {
            run.done.clamp(0.0, 1.0)
        };
        let report = Report {
            agent: self.agent,
            epoch: terms.epoch,
            row,
            measured: run.measured,
            done,
            health: on_time(terms.time_limit, &run.measured).min(done),
            over_budget: terms.over_budget,
            stalled: run.stalled,
            workload: None,
            switch,
        };
        self.last = LastRun {
            report: Some(report.clone()),
            costs: run.costs,
            message: run.message,
        };
        report
    }

    /// What the installed layer competes for; nothing while none is.
    pub fn contention(&self) -> Contention {
        self.installed
            .as_ref()
            .map_or_else(Contention::none, |held| held.layer.contention())
    }

    /// Shuts down every layer the place holds — installed, staged and kept —
    /// once each.
    pub fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        let held = [self.installed.take(), self.staged.take()];
        for mut held in held.into_iter().flatten() {
            held.layer.shutdown(gpu);
        }
        for mut kept in self.kept.drain(..) {
            kept.layer.shutdown(gpu);
        }
    }

    /// Keeps `layer` ready for [`KEEP_EPOCHS`] epochs.
    fn keep(&mut self, layer: Box<dyn Layer>) {
        self.kept.push(Kept {
            layer,
            epochs: KEEP_EPOCHS,
        });
    }

    /// Whether the installed layer and the kept ones fit `envelope`'s memory
    /// on every device.
    fn memory_fits(&self, envelope: &Envelope) -> bool {
        let held = self
            .installed
            .iter()
            .map(|held| held.layer.memory())
            .chain(self.kept.iter().map(|kept| kept.layer.memory()))
            .fold(PerDevice::<u64>::default(), |sum, memory| PerDevice {
                cpu: sum.cpu.saturating_add(memory.cpu),
                gpu: sum.gpu.saturating_add(memory.gpu),
                npu: sum.npu.saturating_add(memory.npu),
                other: sum.other.saturating_add(memory.other),
            });
        let room = &envelope.memory;
        held.cpu <= room.cpu
            && held.gpu <= room.gpu
            && held.npu <= room.npu
            && held.other <= room.other
    }
}

/// How well a run kept to `limit`: the limit over the busiest device's time,
/// at most `1.0`; `1.0` when no time was lent or none was taken.
fn on_time(limit: Duration, measured: &PerDevice<Duration>) -> f32 {
    let took = measured
        .cpu
        .max(measured.gpu)
        .max(measured.npu)
        .max(measured.other);
    if limit.is_zero() || took.is_zero() {
        return 1.0;
    }
    (limit.as_secs_f32() / took.as_secs_f32()).min(1.0)
}
