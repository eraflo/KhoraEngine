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

//! The audio family's agent: its registration, and nothing else.
//!
//! The engine boots one [`LaneAgent`](crate::lane_agent::LaneAgent) from it.
//! The agent offers the [`SpatialMixingLane`]'s three voice levels to GORNA,
//! fetches the frame's `AudioView` and the shared mix bus, and commits the
//! playback updates. The audio device is opened by the application during
//! bootstrap; its stream drains the mix bus on its own real-time thread.
//!
//! [`SpatialMixingLane`]: khora_lanes::audio_lane::SpatialMixingLane

mod agent;
