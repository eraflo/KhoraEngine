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

//! The UI family's agent: its registration, and nothing else.
//!
//! The engine boots one [`LaneAgent`](crate::lane_agent::LaneAgent) from it
//! in the OUTPUT phase. Each frame the agent lends the
//! [`UiLane`](khora_lanes::ui_lane::UiLane) the frame's target, the UI Flow's
//! `UiScene`, the image atlas and the text renderer, and commits its pass
//! into [`UiPassSlot`](khora_data::render::UiPassSlot).

mod agent;
