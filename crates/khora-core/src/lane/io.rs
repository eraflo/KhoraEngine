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

//! A lane's typed inputs and outputs, and where they come from.
//!
//! A lane names its inputs as a struct deriving `LaneInputs` and its family's
//! outputs as a struct deriving `LaneOutputs`. Each field carries an attribute
//! saying where it comes from (`#[bus]`, `#[resource]`, `#[locked]`, …), so
//! the derive knows what the lane touches: its [`Declaration`], from which its
//! [`Contention`] follows. A lane never sees the `World`, the whole deck or the
//! whole runtime — only the fields it named, fetched from an [`InputSource`].

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use super::context_keys::ClearColor;
use super::guard::{
    InputRefusal, InputWarning, OutputRefusal, Reason, SinkKind, SourceKind, Validate,
};
use super::{LaneBus, LaneError, OutputDeck};
use crate::agent::Contention;
use crate::math::Extent2D;
use crate::renderer::api::command::CommandBufferId;
use crate::renderer::api::resource::{TextureFormat, TextureViewId};
use crate::renderer::traits::CommandEncoder;
use crate::renderer::GraphicsDevice;
use crate::runtime::Runtime;

/// How long a fetch waits for its locks when the lane was given no budget.
pub const LOCK_WAIT: Duration = Duration::from_millis(2);

/// The longest a fetch waits for its locks, whatever its budget: a lock not
/// obtained in a quarter of a second is a stall, not a wait.
pub const MAX_LOCK_WAIT: Duration = Duration::from_millis(250);

/// A frame's number, stamped on what is published for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(pub u64);

/// What a target is, for a lane building pipelines for it: when it changes the
/// lane is prepared again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TargetKey {
    /// The colour attachment's format.
    pub color: TextureFormat,
    /// The depth attachment's format, if it has one.
    pub depth: Option<TextureFormat>,
    /// Samples per pixel.
    pub samples: u32,
}

/// The target a lane draws into this frame.
#[derive(Debug, Clone, Copy)]
pub struct Target {
    /// The colour attachment.
    pub color: TextureViewId,
    /// The depth attachment, if it has one.
    pub depth: Option<TextureViewId>,
    /// Its size in pixels.
    pub size: Extent2D,
    /// Its formats and samples.
    pub key: TargetKey,
}

/// The simulated time one run of a fixed-step lane advances: its agent's
/// step, so the lane integrates by exactly the time the scheduler cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedStep(pub Duration);

/// The share of the frame a lane was given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budget {
    /// The time the lane may spend.
    pub time: Duration,
}

/// A runtime entry registered as `Arc<Mutex<T>>`, locked for one record.
///
/// Declared as a lock of the registered `Arc<Mutex<T>>`.
pub struct Locked<'f, T: ?Sized> {
    guard: MutexGuard<'f, T>,
}

impl<'f, T: ?Sized> Locked<'f, T> {
    /// Wraps the guard of a lock taken for one record.
    pub fn new(guard: MutexGuard<'f, T>) -> Self {
        Self { guard }
    }
}

impl<T: ?Sized> std::ops::Deref for Locked<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.guard
    }
}

impl<T: ?Sized> std::ops::DerefMut for Locked<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.guard
    }
}

/// The encoder of pass slot `P`, opened for one record and closed on commit.
///
/// Opened on the device the agent lends its sink, with the slot's
/// descriptor; the lane records into [`encoder`](Self::encoder), may amend
/// the descriptor, and the commit finishes the encoder into the slot. The
/// engine submits the slot's command buffer in the registered pass order —
/// a lane never submits.
pub struct Pass<'f, P: PassSlot> {
    encoder: Box<dyn CommandEncoder>,
    device: &'f dyn GraphicsDevice,
    descriptor: P::Descriptor,
}

/// A pass slot a `#[pass]` output closes into: what the frame graph orders
/// the pass by, and how the slot holds the finished pass.
///
/// khora-core cannot name the frame graph's types; the slot, defined beside
/// them, says what its descriptor is and how it is built.
pub trait PassSlot: Default + Any + Send + Sync {
    /// What the frame graph orders the pass by. The lane may amend it while
    /// it records.
    type Descriptor: Send;

    /// The descriptor a pass of this slot opens with.
    fn descriptor() -> Self::Descriptor;

    /// The slot holding the closed pass: its descriptor and the command
    /// buffer its encoder finished into.
    fn close(descriptor: Self::Descriptor, command_buffer: CommandBufferId) -> Self;
}

impl<'f, P: PassSlot> Pass<'f, P> {
    /// Opens a pass of slot `P` on `device`.
    pub fn open(device: &'f dyn GraphicsDevice) -> Self {
        Self {
            encoder: device.create_command_encoder(Some(std::any::type_name::<P>())),
            device,
            descriptor: P::descriptor(),
        }
    }

    /// The encoder the lane records the pass into.
    pub fn encoder(&mut self) -> &mut dyn CommandEncoder {
        self.encoder.as_mut()
    }

    /// The device the pass records for: what the lane uploads its per-pass
    /// uniforms through.
    pub fn device(&self) -> &'f dyn GraphicsDevice {
        self.device
    }

    /// The descriptor the pass closes with.
    pub fn descriptor_mut(&mut self) -> &mut P::Descriptor {
        &mut self.descriptor
    }

    /// Finishes the encoder into the slot; `None` when the encoder gives no
    /// command buffer.
    pub fn close(self) -> Option<P> {
        let descriptor = self.descriptor;
        self.encoder
            .finish()
            .map(|command_buffer| P::close(descriptor, command_buffer))
    }
}

/// One source or sink a lane named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Declared<K> {
    /// The field's name.
    pub field: &'static str,
    /// Where it is read or written.
    pub kind: K,
    /// The type looked up: the View, the slot, the registered runtime entry
    /// (`Arc<Mutex<T>>` for a `Locked<'f, T>`).
    pub ty: TypeId,
    /// That type's name, for messages.
    pub type_name: &'static str,
}

impl<K> Declared<K> {
    fn of<T: ?Sized + 'static>(field: &'static str, kind: K) -> Self {
        Self {
            field,
            kind,
            ty: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
        }
    }
}

/// Everything a lane reads and writes, as its derives declare it.
///
/// `Declaration::default()` is the empty declaration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Declaration {
    inputs: Vec<Declared<SourceKind>>,
    outputs: Vec<Declared<SinkKind>>,
}

impl Declaration {
    /// Declares the input `field`, read from `kind` as the type `T`.
    pub fn read<T: ?Sized + 'static>(
        &mut self,
        field: &'static str,
        kind: SourceKind,
    ) -> &mut Self {
        self.inputs.push(Declared::of::<T>(field, kind));
        self
    }

    /// Declares the output `field`, written to `kind` as the type `T`.
    pub fn write<T: ?Sized + 'static>(&mut self, field: &'static str, kind: SinkKind) -> &mut Self {
        self.outputs.push(Declared::of::<T>(field, kind));
        self
    }

    /// Adds every input and output of `other`.
    pub fn extend(&mut self, other: Declaration) -> &mut Self {
        self.inputs.extend(other.inputs);
        self.outputs.extend(other.outputs);
        self
    }

    /// The inputs declared, in field order.
    pub fn inputs(&self) -> &[Declared<SourceKind>] {
        &self.inputs
    }

    /// The outputs declared, in field order.
    pub fn outputs(&self) -> &[Declared<SinkKind>] {
        &self.outputs
    }

    /// What the declaration contends for: `#[resource]` and `#[resident]`
    /// reads, `#[locked]` locks, and the slots `#[deck]` and `#[pass]`
    /// outputs write. A View, a deck read,
    /// the frame and the budget are nobody else's to write while the lane
    /// runs, so they contend for nothing.
    pub fn contention(&self) -> Contention {
        let mut contention = Contention::none();
        for input in &self.inputs {
            let into = match input.kind {
                SourceKind::Resource | SourceKind::Resident => &mut contention.reads,
                SourceKind::Locked => &mut contention.writes,
                _ => continue,
            };
            if !into.contains(&input.ty) {
                into.push(input.ty);
            }
        }
        for output in &self.outputs {
            if !contention.deck.contains(&output.ty) {
                contention.deck.push(output.ty);
            }
        }
        contention
    }
}

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
    /// budget ([`LOCK_WAIT`] without one, at most [`MAX_LOCK_WAIT`]) from the
    /// first wait. A lock held elsewhere — or already held by this very
    /// fetch — is refused as `LockTimedOut` rather than blocking the lane and
    /// its worker.
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
        let wait = self
            .budget
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

/// Where a lane's outputs go for one frame: the deck slots and pass slots its
/// family declared, committed by the agent after the lane ran.
pub struct OutputSink<'f> {
    deck: &'f mut OutputDeck,
    /// The slot types committed through this sink.
    written: Vec<TypeId>,
    /// What the slots hold this frame so far, outputs start from.
    seed: Option<&'f OutputDeck>,
    /// The device `#[pass]` outputs open their encoder on.
    device: Option<&'f dyn GraphicsDevice>,
}

impl<'f> OutputSink<'f> {
    /// A sink committing into `deck`.
    pub fn new(deck: &'f mut OutputDeck) -> Self {
        Self {
            deck,
            written: Vec::new(),
            seed: None,
            device: None,
        }
    }

    /// The same sink, its deck outputs opening on what `seed` holds this
    /// frame: a lane run once per fixed sub-step adds to what the earlier
    /// sub-steps wrote, and one that writes the whole value replaces it.
    pub fn with_seed(mut self, seed: &'f OutputDeck) -> Self {
        self.seed = Some(seed);
        self
    }

    /// The same sink, its `#[pass]` outputs opening an encoder on `device`.
    pub fn with_device(mut self, device: &'f dyn GraphicsDevice) -> Self {
        self.device = Some(device);
        self
    }

    /// The device `#[pass]` outputs open on, when the agent lent one.
    pub fn device(&self) -> Option<&'f dyn GraphicsDevice> {
        self.device
    }

    /// What the slot `T` holds this frame so far, or its default.
    pub fn opening<T: Default + Clone + Any + Send + Sync>(&self) -> T {
        self.seed
            .and_then(|seed| seed.get::<T>())
            .cloned()
            .unwrap_or_default()
    }

    /// Writes `value` into the deck slot `T`. A slot holds one value a frame:
    /// a second write through the same sink is refused, naming `field`, and
    /// the first value stays.
    pub fn put<T: Default + Any + Send + Sync>(
        &mut self,
        field: &'static str,
        value: T,
    ) -> Result<(), OutputRefusal> {
        self.write(field, SinkKind::Deck, value)
    }

    /// Writes a closed pass into its slot `P`, refused as
    /// [`put`](Self::put) refuses a second write.
    pub fn put_pass<P: PassSlot>(
        &mut self,
        field: &'static str,
        pass: P,
    ) -> Result<(), OutputRefusal> {
        self.write(field, SinkKind::Pass, pass)
    }

    fn write<T: Default + Any + Send + Sync>(
        &mut self,
        field: &'static str,
        sink: SinkKind,
        value: T,
    ) -> Result<(), OutputRefusal> {
        let slot = TypeId::of::<T>();
        if self.written.contains(&slot) {
            return Err(OutputRefusal {
                field,
                sink,
                reason: Reason::WrittenTwice,
            });
        }
        self.written.push(slot);
        *self.deck.slot::<T>() = value;
        Ok(())
    }
}

/// A lane's inputs, derived with `#[derive(LaneInputs)]`.
pub trait LaneInputs<'f>: Sized {
    /// Declares every field's source.
    fn declare(d: &mut Declaration);

    /// Fetches every field from `src`, running the guards.
    fn fetch(src: &mut InputSource<'f>) -> Result<Self, InputRefusal>;

    /// The target the inputs draw into, when they have one.
    fn target_key(&self) -> Option<TargetKey> {
        None
    }
}

/// A family's outputs, derived with `#[derive(LaneOutputs)]`.
pub trait LaneOutputs<'f>: Sized {
    /// Declares every field's sink.
    fn declare(d: &mut Declaration);

    /// Opens every field for one record.
    fn open(sink: &mut OutputSink<'f>) -> Result<Self, LaneError>;

    /// Commits every field into `sink`, running the output guards.
    fn commit(self, sink: &mut OutputSink<'f>) -> Result<(), OutputRefusal>;
}
