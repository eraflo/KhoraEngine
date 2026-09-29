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

//! One frame of rendering: the render context handed to a frame, and the
//! per-frame state the stages share.

pub mod frame_context;
pub mod render_context;

pub use self::frame_context::{FrameContext, StageHandle};
pub use self::render_context::*;

/// The maximum number of frames that can be processed by the GPU at once.
/// This determines the number of slots in ring buffers and other per-frame resources.
pub const MAX_FRAMES_IN_FLIGHT: usize = 2;
