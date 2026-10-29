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

use std::sync::Arc;

use khora_core::lane::LaneContext;
use khora_core::renderer::GraphicsDevice;

/// The device a lane releases its resources on, when the shutdown context
/// carries one.
pub(crate) fn shutdown_device(ctx: &LaneContext) -> Option<Arc<dyn GraphicsDevice>> {
    ctx.get::<Arc<dyn GraphicsDevice>>().cloned()
}
