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

//! Where a lane's inputs are fetched from for one frame.

use std::any::Any;
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use super::{Budget, Declaration, FixedStep, FrameId, Locked, SceneTarget, Target};
use crate::event::Supersedes;
use crate::lane::context_keys::ClearColor;
use crate::lane::guard::{InputRefusal, InputWarning, Reason, SourceKind, Validate};
use crate::lane::{LaneBus, OutputDeck};
use crate::runtime::Runtime;

/// How long a fetch waits for its locks when the lane was given no budget.
pub const LOCK_WAIT: Duration = Duration::from_millis(2);

/// The longest a fetch waits for its locks, whatever its budget: a lock not
/// obtained in a quarter of a second is a stall, not a wait.
pub const MAX_LOCK_WAIT: Duration = Duration::from_millis(250);

/// Where a lane's inputs are fetched from for one frame.
///
/// It holds the bus, the earlier phases' deck, the runtime, the target and the
/// budget — and no `World`: a lane reads the world only through the Views a
/// Flow published.
pub struct InputSource<'f> {
    frame: FrameId,
    bus: &'f LaneBus,
    runtime: &'f Runtime,
    deck: Option<&'f OutputDeck>,
    target: Option<Target>,
    clear_color: Option<ClearColor>,
    budget: Option<Budget>,
    step: Option<FixedStep>,
    /// When the fetch stops waiting for locks: set by the first lock it
    /// waits for, shared by every later one.
    lock_deadline: Option<Instant>,
    /// Whether a lock held elsewhere is waited for; a quote never waits.
    waits: bool,
    /// Whether a `#[take]` drains its channel: a run does; a quote or a
    /// preparation reads what it holds and leaves it for the run.
    takes: bool,
    /// How to give back what each `#[take]` of the fetch under way drained,
    /// should a later field be refused.
    taken: Vec<(&'f dyn crate::event::Restore, Box<dyn std::any::Any + Send>)>,
    touched: Declaration,
    warnings: Vec<InputWarning>,
}

impl<'f> InputSource<'f> {
    /// Returns a source for `frame`, reading `bus` and `runtime`.
    pub fn new(frame: FrameId, bus: &'f LaneBus, runtime: &'f Runtime) -> Self {
        Self {
            frame,
            bus,
            runtime,
            deck: None,
            target: None,
            clear_color: None,
            budget: None,
            step: None,
            lock_deadline: None,
            waits: true,
            takes: true,
            taken: Vec::new(),
            touched: Declaration::default(),
            warnings: Vec::new(),
        }
    }

    /// Adds the deck the earlier phases wrote.
    pub fn with_deck(mut self, deck: &'f OutputDeck) -> Self {
        self.deck = Some(deck);
        self
    }

    /// Adds the frame's target.
    pub fn with_target(mut self, target: Target) -> Self {
        self.target = Some(target);
        self
    }

    /// Adds the colour the frame's target is cleared to.
    pub fn with_clear_color(mut self, clear_color: ClearColor) -> Self {
        self.clear_color = Some(clear_color);
        self
    }

    /// Adds the step a fixed-step agent runs its lane at.
    pub fn with_step(mut self, step: FixedStep) -> Self {
        self.step = Some(step);
        self
    }

    /// Adds the lane's budget.
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Sets whether a [`lock`](Self::lock) held elsewhere is waited for; returns the old value.
    ///
    /// Not waiting, a held lock is refused at once as `LockTimedOut`, and no
    /// deadline is started.
    pub(crate) fn set_waiting(&mut self, waits: bool) -> bool {
        std::mem::replace(&mut self.waits, waits)
    }

    /// Keeps what the fetch's `#[take]`s drained: the fetch succeeded, and the
    /// lane has the requests.
    pub fn keep_takes(&mut self) {
        self.taken.clear();
    }

    /// Gives back what the fetch's `#[take]`s drained, at the front of their
    /// channels: a later field was refused, and the lane never saw them.
    pub fn restore_takes(&mut self) {
        for (channel, items) in self.taken.drain(..).rev() {
            channel.restore_any(items);
        }
    }

    /// Sets whether a `#[take]` drains its channel; returns the old value.
    ///
    /// A quote or a preparation reads the requests and leaves them for the run.
    pub(crate) fn set_taking(&mut self, takes: bool) -> bool {
        std::mem::replace(&mut self.takes, takes)
    }

    /// Returns the frame the source is for.
    pub fn frame(&self) -> FrameId {
        self.frame
    }

    /// Returns every source a fetch reached so far.
    pub fn touched(&self) -> Declaration {
        self.touched.clone()
    }

    /// Returns the warnings fetches raised so far.
    pub fn warnings(&self) -> &[InputWarning] {
        &self.warnings
    }

    /// Records that `field` reached `kind` for the type `T`.
    fn touch<T: ?Sized + 'static>(&mut self, field: &'static str, kind: SourceKind) {
        self.touched.read::<T>(field, kind);
    }

    /// Returns the View `V`, published for this frame.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` when the bus holds no `V`, as `Stale` when it was
    /// published for another frame.
    pub fn view<V: Any + Send + Sync>(
        &mut self,
        field: &'static str,
    ) -> Result<&'f V, InputRefusal> {
        self.touch::<V>(field, SourceKind::Bus);
        let refuse = |reason| InputRefusal {
            field,
            source: SourceKind::Bus,
            reason,
        };
        let bus = self.bus;
        let view = bus.get::<V>().ok_or(refuse(Reason::Missing))?;
        if bus.stamp_of::<V>() != Some(self.frame) {
            return Err(refuse(Reason::Stale));
        }
        Ok(view)
    }

    /// Returns the slot `T` an earlier phase wrote, if it did.
    pub fn deck_slot<T: Any + Send + Sync>(&mut self, field: &'static str) -> Option<&'f T> {
        self.touch::<T>(field, SourceKind::Deck);
        self.deck.and_then(|deck| deck.get::<T>())
    }

    /// Returns the runtime entry `T`, read without locking.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` when the runtime holds no `T`.
    pub fn entry<T: Send + Sync + 'static>(
        &mut self,
        field: &'static str,
        kind: SourceKind,
    ) -> Result<&'f T, InputRefusal> {
        self.touch::<T>(field, kind);
        let runtime = self.runtime;
        runtime.look_up::<T>().ok_or(InputRefusal {
            field,
            source: kind,
            reason: Reason::Missing,
        })
    }

    /// Returns the runtime entry registered as `Arc<Mutex<T>>`, locked.
    ///
    /// Every lock of one fetch shares one deadline from its first wait: the
    /// lane's budget, or [`LOCK_WAIT`] without a positive one, at most
    /// [`MAX_LOCK_WAIT`]. A lock held elsewhere, or by this very fetch, is
    /// refused rather than blocking the worker.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` when the runtime holds no such entry, as
    /// `LockPoisoned`, or as `LockTimedOut` past the deadline (at once on a
    /// source that does not wait).
    pub fn lock<T: ?Sized + Send + 'static>(
        &mut self,
        field: &'static str,
    ) -> Result<Locked<'f, T>, InputRefusal> {
        let shared = self.entry::<Arc<Mutex<T>>>(field, SourceKind::Locked)?;
        let refuse = |reason| InputRefusal {
            field,
            source: SourceKind::Locked,
            reason,
        };
        if !self.waits {
            return match shared.try_lock() {
                Ok(guard) => Ok(Locked::new(guard)),
                Err(TryLockError::Poisoned(_)) => Err(refuse(Reason::LockPoisoned)),
                Err(TryLockError::WouldBlock) => Err(refuse(Reason::LockTimedOut)),
            };
        }
        // A budget of no time buys no work, not no wait: a lane lent nothing
        // waits as one never lent.
        let wait = self
            .budget
            .filter(|budget| !budget.time.is_zero())
            .map_or(LOCK_WAIT, |budget| budget.time)
            .min(MAX_LOCK_WAIT);
        let deadline = *self
            .lock_deadline
            .get_or_insert_with(|| Instant::now() + wait);
        loop {
            match shared.try_lock() {
                Ok(guard) => return Ok(Locked::new(guard)),
                Err(TryLockError::Poisoned(_)) => return Err(refuse(Reason::LockPoisoned)),
                Err(TryLockError::WouldBlock) if Instant::now() >= deadline => {
                    return Err(refuse(Reason::LockTimedOut))
                }
                Err(TryLockError::WouldBlock) => std::thread::yield_now(),
            }
        }
    }

    /// Drains every request the runtime's `Channel<T>` holds, for a `#[take]` field.
    ///
    /// A source not taking (a quote, a preparation) reads them and leaves them.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` when the runtime holds no such channel.
    pub fn take<T: Supersedes + Clone + Send + Sync + 'static>(
        &mut self,
        field: &'static str,
    ) -> Result<Vec<T>, InputRefusal> {
        let takes = self.takes;
        let channel = self.entry::<crate::event::Channel<T>>(field, SourceKind::Take)?;
        if !takes {
            return Ok(channel.read(&mut crate::event::Cursor::default()));
        }
        let drained = channel.drain();
        if !drained.is_empty() {
            self.taken.push((channel, Box::new(drained.clone())));
        }
        Ok(drained)
    }

    /// Puts `items` left in a `#[take]` field back at the front of `Channel<T>`, in order.
    ///
    /// An item a queued one supersedes is dropped.
    pub fn give_back<T: Supersedes + Send + Sync + 'static>(&mut self, items: Vec<T>) {
        // A source that does not take drained nothing, so owes nothing back.
        if items.is_empty() || !self.takes {
            return;
        }
        match self.runtime.look_up::<crate::event::Channel<T>>() {
            Some(channel) => channel.restore(items),
            None => log::error!(
                "{} {} given back to a runtime that holds no channel for them are lost",
                items.len(),
                std::any::type_name::<T>()
            ),
        }
    }

    /// Returns the frame's target, when it has a size.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` without a target, as `BadTarget` when it has no size.
    pub fn target(&mut self, field: &'static str) -> Result<Target, InputRefusal> {
        self.touch::<Target>(field, SourceKind::Frame);
        let refuse = |reason| InputRefusal {
            field,
            source: SourceKind::Frame,
            reason,
        };
        let target = self.target.ok_or(refuse(Reason::Missing))?;
        if target.size.width == 0 || target.size.height == 0 {
            return Err(refuse(Reason::BadTarget));
        }
        Ok(target)
    }

    /// Returns the frame's target, when it has a size and a depth attachment.
    ///
    /// # Errors
    ///
    /// As [`target`](Self::target), and `BadTarget` without a depth attachment.
    pub(crate) fn scene_target(
        &mut self,
        field: &'static str,
    ) -> Result<SceneTarget, InputRefusal> {
        let target = self.target(field)?;
        let depth = target.depth.ok_or(InputRefusal {
            field,
            source: SourceKind::Frame,
            reason: Reason::BadTarget,
        })?;
        Ok(SceneTarget {
            color: target.color,
            depth,
            size: target.size,
            key: target.key,
        })
    }

    /// Returns the colour the frame's target is cleared to.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` when the source has none.
    pub fn clear_color(&mut self, field: &'static str) -> Result<ClearColor, InputRefusal> {
        self.touch::<ClearColor>(field, SourceKind::Frame);
        self.clear_color.ok_or(InputRefusal {
            field,
            source: SourceKind::Frame,
            reason: Reason::Missing,
        })
    }

    /// Returns the step the lane's agent runs it at.
    ///
    /// # Errors
    ///
    /// Refused as `Missing` when the family is not fixed-step.
    pub fn fixed_step(&mut self, field: &'static str) -> Result<FixedStep, InputRefusal> {
        self.touch::<FixedStep>(field, SourceKind::Frame);
        self.step.ok_or(InputRefusal {
            field,
            source: SourceKind::Frame,
            reason: Reason::Missing,
        })
    }

    /// Returns the budget the lane was lent, zero included; `None` when its agent was never
    /// budgeted.
    pub(crate) fn lent_budget(&mut self, field: &'static str) -> Option<Budget> {
        self.touch::<Budget>(field, SourceKind::Budget);
        self.budget
    }

    /// Returns the lane's budget, when it has a positive one.
    ///
    /// # Errors
    ///
    /// Refused as `NoBudget` without one, or with one of no time.
    pub fn budget(&mut self, field: &'static str) -> Result<Budget, InputRefusal> {
        self.touch::<Budget>(field, SourceKind::Budget);
        self.budget
            .filter(|budget| !budget.time.is_zero())
            .ok_or(InputRefusal {
                field,
                source: SourceKind::Budget,
                reason: Reason::NoBudget,
            })
    }

    /// Runs `value`'s own check, keeping a warning when it finds a problem.
    /// The value is never rewritten and never refused.
    pub fn validate<V: Validate + ?Sized>(
        &mut self,
        field: &'static str,
        source: SourceKind,
        value: &V,
    ) {
        let problems = value.validate();
        if !problems.is_empty() {
            self.warnings.push(InputWarning {
                field,
                source,
                problems,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::LinearRgba;

    /// **The clear colour is read from the source that was given one**, and
    /// refused as `Missing` by one that was not.
    #[test]
    fn the_clear_color_is_read_only_from_a_source_given_one() {
        let bus = LaneBus::new();
        let runtime = Runtime::new();
        let red = LinearRgba::RED;

        let mut given =
            InputSource::new(bus.frame(), &bus, &runtime).with_clear_color(ClearColor(red));
        let mut bare = InputSource::new(bus.frame(), &bus, &runtime);

        assert_eq!(given.clear_color("clear").map(|c| c.0), Ok(red));
        assert_eq!(
            bare.clear_color("clear").map(|c| c.0),
            Err(InputRefusal {
                field: "clear",
                source: SourceKind::Frame,
                reason: Reason::Missing,
            })
        );
    }
}
