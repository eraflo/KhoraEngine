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

//! Render-domain lanes (the hot-path strategies for the rendering subsystem).
//!
//! The per-frame `RenderWorld` and the extraction logic live in
//! [`khora_data::render`].  This module exposes the lanes that consume that
//! data and the UI-scene types specific to the UI render pipeline.

mod forward_plus_lane;
mod lit_forward_lane;
pub mod shaders;
mod shadow_pass_lane;
mod simple_unlit_lane;
mod ui_render_lane;
pub mod util;

pub use forward_plus_lane::*;
pub use lit_forward_lane::*;
pub use shadow_pass_lane::*;
pub use simple_unlit_lane::*;
pub use ui_render_lane::*;
