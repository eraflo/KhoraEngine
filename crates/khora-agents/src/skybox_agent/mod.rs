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

//! The skybox family's agent: its registration, and nothing else.
//!
//! The engine boots one [`LaneAgent`](crate::lane_agent::LaneAgent) from it,
//! after the renderer in the OUTPUT phase. Each frame the agent lends the
//! [`SkyboxLane`](khora_lanes::skybox_lane::SkyboxLane) the frame's target,
//! opens the skybox pass on the device and commits it into
//! [`SkyboxPassSlot`](khora_data::render::SkyboxPassSlot)
//! (`writes(Color).reads(Depth)`), which the FrameGraph folds between the
//! scene's opaque and blended passes.

mod agent;
