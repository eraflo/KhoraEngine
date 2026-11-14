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

use std::any::TypeId;
use std::sync::Arc;

use crate::agent::Contention;
use crate::lane::context_keys::ClearColor;
use crate::lane::guard::{InputRefusal, SinkKind, SourceKind};
use crate::math::Extent2D;
use crate::renderer::api::frame::FrameContext;
use crate::renderer::api::resource::{TextureFormat, TextureViewId};

mod budget;
mod fixed_step;
mod frame_id;
mod input_source;
mod lane_outputs;
mod locked;
mod output_sink;
mod pass;
mod scene_target;

pub use budget::Budget;
pub use fixed_step::FixedStep;
pub use frame_id::FrameId;
pub use input_source::{InputSource, LOCK_WAIT, MAX_LOCK_WAIT};
pub use lane_outputs::LaneOutputs;
pub use locked::Locked;
pub use output_sink::OutputSink;
pub use pass::{Pass, PassSlot};
pub use scene_target::SceneTarget;

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
                SourceKind::Deck => (&mut contention.deck_reads, input.ty),
                // Draining a channel empties it for every other reader: a
                // take is a lock of the channel.
                SourceKind::Locked | SourceKind::Take => (&mut contention.writes, input.ty),
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
            // A staged output goes into the run's own staging, then the
            // reserve: no slot of the frame's deck.
            if output.kind == SinkKind::Staged {
                continue;
            }
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

    /// Gives back to `src` what the run left in each `#[take]` field: what
    /// the lane did not consume, it did not load. Nothing by default.
    fn give_back(self, src: &mut InputSource<'f>) {
        let _ = src;
    }
}
