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

mod emissive_lane;
mod forward_plus_lane;
mod gizmo_lane;
mod grid_lane;
mod lit_forward_lane;
pub mod shaders;
pub mod shadows_lane;
mod simple_unlit_lane;
mod standard_pbr_lane;
mod ui_render_lane;
pub mod util;
mod wireframe_lane;

pub use emissive_lane::EmissiveLane;
pub use forward_plus_lane::*;
pub use gizmo_lane::{GizmoLane, SharedGizmoFrame};
pub use grid_lane::{GridLane, SharedGridConfig};
pub use lit_forward_lane::*;
pub use shadows_lane::{LowResShadowsLane, MediumShadowsLane, StandardShadowsLane};
pub use simple_unlit_lane::*;
pub use standard_pbr_lane::StandardPbrLane;
pub use ui_render_lane::*;
pub use wireframe_lane::WireframeLane;
