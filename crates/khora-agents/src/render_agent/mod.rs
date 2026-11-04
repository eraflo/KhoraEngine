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

//! The render family's agent: its registration, and nothing else.
//!
//! The engine boots one [`LaneAgent`](crate::agent::LaneAgent) from it,
//! offering three ways to draw the scene to GORNA: the unlit lane, the forward
//! lane and Forward+ (on a device with storage buffers and compute). Each
//! frame the agent hands the chosen lane the bus's `RenderWorld`, the frame's
//! target and clear colour, the shadows family's frame and the IBL bake, opens
//! the scene and transparent passes on the device, and commits them closed.

mod agent;

pub use agent::*;
