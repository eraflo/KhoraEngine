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

//! Where a lane's outputs go for one frame.

use std::any::{Any, TypeId};

use super::PassSlot;
use crate::lane::guard::{OutputRefusal, Reason, SinkKind};
use crate::lane::reserve::Staged;
use crate::lane::staging::Staging;
use crate::lane::OutputDeck;
use crate::renderer::traits::PipelineSystem;
use crate::renderer::GraphicsDevice;

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
    /// The pipeline system `#[pass]` outputs lend the lane.
    pipelines: Option<&'f dyn PipelineSystem>,
    /// Where `#[staged]` outputs go: the staging of a background run.
    staging: Option<&'f mut Staging>,
}

impl<'f> OutputSink<'f> {
    /// A sink committing into `deck`.
    pub fn new(deck: &'f mut OutputDeck) -> Self {
        Self {
            deck,
            written: Vec::new(),
            seed: None,
            device: None,
            pipelines: None,
            staging: None,
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

    /// The same sink, its `#[pass]` outputs lending `pipelines` to the lane
    /// that records into them.
    pub fn with_pipelines(mut self, pipelines: &'f dyn PipelineSystem) -> Self {
        self.pipelines = Some(pipelines);
        self
    }

    /// The pipeline system `#[pass]` outputs lend, when the agent lent one.
    pub fn pipelines(&self) -> Option<&'f dyn PipelineSystem> {
        self.pipelines
    }

    /// The same sink, its `#[staged]` outputs going into `staging`: what one
    /// run staged, handed to the family's reserve once the run is over.
    pub fn with_staging(mut self, staging: &'f mut Staging) -> Self {
        self.staging = Some(staging);
        self
    }

    /// Stages `items` into the run's staging. Refused as `Missing`, naming
    /// `field`, by a sink lent no staging.
    pub fn stage<T: Staged>(
        &mut self,
        field: &'static str,
        items: Vec<T>,
    ) -> Result<(), OutputRefusal> {
        let Some(staging) = self.staging.as_deref_mut() else {
            return Err(OutputRefusal {
                field,
                sink: SinkKind::Staged,
                reason: Reason::Missing,
            });
        };
        staging.stage(items);
        Ok(())
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
