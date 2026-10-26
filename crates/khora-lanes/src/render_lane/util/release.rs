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

//! Releasing, at shutdown, what a lane created on the device.
//!
//! A lane destroys exactly what it asked the device for — buffers, bind
//! groups, samplers. Layouts and pipelines come from the `PipelineSystem`,
//! which owns and shares them across lanes, so a lane never destroys those.

use std::sync::{Arc, OnceLock};

use khora_core::lane::LaneContext;
use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::api::resource::{BufferId, SamplerId};
use khora_core::renderer::GraphicsDevice;

/// The device a lane releases its resources on, when the shutdown context
/// carries one.
pub(crate) fn shutdown_device(ctx: &LaneContext) -> Option<Arc<dyn GraphicsDevice>> {
    ctx.get::<Arc<dyn GraphicsDevice>>().cloned()
}

/// Destroys the buffer held in `slot`, if initialisation created one.
pub(crate) fn release_buffer(device: &dyn GraphicsDevice, slot: &OnceLock<BufferId>) {
    if let Some(id) = slot.get() {
        if let Err(e) = device.destroy_buffer(*id) {
            log::warn!("lane shutdown: could not destroy buffer {id:?}: {e}");
        }
    }
}

/// Destroys the bind group held in `slot`, if initialisation created one.
pub(crate) fn release_bind_group(device: &dyn GraphicsDevice, slot: &OnceLock<BindGroupId>) {
    if let Some(id) = slot.get() {
        if let Err(e) = device.destroy_bind_group(*id) {
            log::warn!("lane shutdown: could not destroy bind group {id:?}: {e}");
        }
    }
}

/// Destroys the sampler held in `slot`, if initialisation created one.
pub(crate) fn release_sampler(device: &dyn GraphicsDevice, slot: &OnceLock<SamplerId>) {
    if let Some(id) = slot.get() {
        if let Err(e) = device.destroy_sampler(*id) {
            log::warn!("lane shutdown: could not destroy sampler {id:?}: {e}");
        }
    }
}
