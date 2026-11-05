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
use std::sync::{Arc, MutexGuard};
use std::time::Duration;

use super::context_keys::ClearColor;
use super::guard::{InputRefusal, OutputRefusal, SinkKind, SourceKind};
use super::LaneError;
use crate::agent::Contention;
use crate::math::Extent2D;
use crate::renderer::api::command::CommandBufferId;
use crate::renderer::api::frame::FrameContext;
use crate::renderer::api::resource::{TextureFormat, TextureViewId};
use crate::renderer::traits::{CommandEncoder, PipelineSystem};
use crate::renderer::GraphicsDevice;

mod input_source;
mod output_sink;

pub use input_source::{InputSource, LOCK_WAIT, MAX_LOCK_WAIT};
pub use output_sink::OutputSink;

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

/// The frame's target, read by a lane that depth-tests what it draws: the
/// target with its depth attachment. A frame whose target has no depth is
/// refused at the fetch, as a [`Reason::BadTarget`](super::guard::Reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTarget {
    /// The colour attachment.
    pub color: TextureViewId,
    /// The depth attachment.
    pub depth: TextureViewId,
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
    pipelines: Option<&'f dyn PipelineSystem>,
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
            pipelines: None,
            descriptor: P::descriptor(),
        }
    }

    /// The same pass, lending `pipelines` to the lane recording into it.
    pub fn lending(mut self, pipelines: Option<&'f dyn PipelineSystem>) -> Self {
        self.pipelines = pipelines;
        self
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

    /// The pipeline system the agent lent the sink the pass opened on, when it
    /// lent one: what a lane resolves the pipelines it draws with through
    /// while it records.
    pub fn pipelines(&self) -> Option<&'f dyn PipelineSystem> {
        self.pipelines
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
    /// reads, `#[locked]` locks, the `FrameContext` a `#[frame]` target or
    /// clear colour is lent from, and the slots `#[deck]` and `#[pass]`
    /// outputs write. A View, a deck read,
    /// the frame and the budget are nobody else's to write while the lane
    /// runs, so they contend for nothing.
    pub fn contention(&self) -> Contention {
        let mut contention = Contention::none();
        let frame_context = TypeId::of::<Arc<FrameContext>>();
        for input in &self.inputs {
            let (into, ty) = match input.kind {
                SourceKind::Resource | SourceKind::Resident => (&mut contention.reads, input.ty),
                SourceKind::Locked => (&mut contention.writes, input.ty),
                // The agent lends the frame's target and clear colour from
                // the `FrameContext`, so reading either reads it.
                SourceKind::Frame
                    if input.ty == TypeId::of::<Target>()
                        || input.ty == TypeId::of::<ClearColor>() =>
                {
                    (&mut contention.reads, frame_context)
                }
                _ => continue,
            };
            if !into.contains(&ty) {
                into.push(ty);
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
