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

//! SkyboxAgent — owns the environment-background render pass.
//!
//! Runs in the OUTPUT phase after `RenderAgent`, like `OverlayAgent`, and
//! contributes a single [`SkyboxPass`](khora_data::render::SkyboxPassSlot) to
//! the FrameGraph (`writes(Color).reads(Depth)`). Its one lane
//! ([`SkyboxLane`](khora_lanes::render_lane::SkyboxLane)) draws the IBL
//! environment cube behind the scene geometry (depth-tested, no depth write),
//! so the visible sky matches what surfaces reflect.
//!
//! It is **not** GORNA-negotiated (there is one fixed background pass), but it
//! is `AgentImportance::Important` — the sky is not debug viz, so it is not
//! dropped under budget pressure the way the optional overlays are.

mod agent;

pub use agent::*;
