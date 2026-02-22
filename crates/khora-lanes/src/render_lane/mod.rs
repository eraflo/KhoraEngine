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

//! Rendering lane - hot path for graphics operations

mod extract_lane;
mod forward_plus_lane;
mod lit_forward_lane;
pub mod shaders;
mod shadow_pass_lane;
mod simple_unlit_lane;
mod world;

pub use extract_lane::*;
pub use forward_plus_lane::*;
pub use lit_forward_lane::*;
pub use shadow_pass_lane::*;
pub use simple_unlit_lane::*;
pub use world::*;

/// Shadow result for a single light: view-projection matrix + atlas layer index.
pub type ShadowResult = (khora_core::math::Mat4, i32);
