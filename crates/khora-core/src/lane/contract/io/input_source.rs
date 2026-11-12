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
    /// A source for `frame`, reading `bus` and `runtime`.
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

    /// Sets whether a [`lock`](Self::lock) held elsewhere is waited for, and
    /// returns what it was. Not waiting, a held lock is refused at once as
    /// `LockTimedOut`, and no deadline is started.
    pub fn set_waiting(&mut self, waits: bool) -> bool {
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

    /// Sets whether a `#[take]` drains its channel, returning what it was:
    /// a quote or a preparation fetches the lane's inputs without taking the
    /// requests a run is for.
    pub fn set_taking(&mut self, takes: bool) -> bool {
        std::mem::replace(&mut self.takes, takes)
    }

    /// The frame the source is for.
    pub fn frame(&self) -> FrameId {
        self.frame
    }

    /// Every source a fetch reached so far.
    pub fn touched(&self) -> Declaration {
        self.touched.clone()
    }

    /// The warnings fetches raised so far.
    pub fn warnings(&self) -> &[InputWarning] {
        &self.warnings
    }

    /// Records that `field` reached `kind` for the type `T`.
    fn touch<T: ?Sized + 'static>(&mut self, field: &'static str, kind: SourceKind) {
        self.touched.read::<T>(field, kind);
    }

    /// The View `V`, published for this frame.
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

    /// The slot `T` an earlier phase wrote, if it did.
    pub fn deck_slot<T: Any + Send + Sync>(&mut self, field: &'static str) -> Option<&'f T> {
        self.touch::<T>(field, SourceKind::Deck);
        self.deck.and_then(|deck| deck.get::<T>())
    }

    /// The runtime entry `T`, read without locking.
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

    /// The runtime entry registered as `Arc<Mutex<T>>`, locked.
    ///
    /// Every lock of one fetch is waited for within one deadline: the lane's
    /// budget ([`LOCK_WAIT`] without one or with one of no time, at most
    /// [`MAX_LOCK_WAIT`]) from the
    /// first wait. A lock held elsewhere — or already held by this very
    /// fetch — is refused as `LockTimedOut` rather than blocking the lane and
    /// its worker; at once, on a source that does not wait.
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

    /// Every request the runtime's `Channel<T>` holds, drained: what a
    /// `#[take]` field reads. A source not taking ([`set_taking`](Self::set_taking))
    /// reads them and leaves them. Refused as `Missing` when the runtime
    /// holds no such channel.
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

    /// Puts `items`, left in a `#[take]` field after a run, back at the front
    /// of the runtime's `Channel<T>`, in their order; an item a queued one
    /// supersedes is dropped.
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

    /// The frame's target, when it has a size.
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

    /// The frame's target, when it has a size and a depth attachment.
    pub fn scene_target(&mut self, field: &'static str) -> Result<SceneTarget, InputRefusal> {
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

    /// The colour the frame's target is cleared to.
    pub fn clear_color(&mut self, field: &'static str) -> Result<ClearColor, InputRefusal> {
        self.touch::<ClearColor>(field, SourceKind::Frame);
        self.clear_color.ok_or(InputRefusal {
            field,
            source: SourceKind::Frame,
            reason: Reason::Missing,
        })
    }

    /// The step the lane's agent runs it at.
    pub fn fixed_step(&mut self, field: &'static str) -> Result<FixedStep, InputRefusal> {
        self.touch::<FixedStep>(field, SourceKind::Frame);
        self.step.ok_or(InputRefusal {
            field,
            source: SourceKind::Frame,
            reason: Reason::Missing,
        })
    }

    /// The budget the lane was lent, zero included; `None` when it was lent
    /// none — its agent was never budgeted.
    pub fn lent_budget(&mut self, field: &'static str) -> Option<Budget> {
        self.touch::<Budget>(field, SourceKind::Budget);
        self.budget
    }

    /// The lane's budget, when it has a positive one.
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
