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

//! The encoder of one pass slot, opened for a lane's record and closed on commit.

use std::any::Any;

use crate::renderer::api::command::CommandBufferId;
use crate::renderer::traits::{CommandEncoder, PipelineSystem};
use crate::renderer::GraphicsDevice;

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
