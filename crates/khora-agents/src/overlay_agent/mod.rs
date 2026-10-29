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

//! The overlay families' agents: their registrations, and nothing else.
//!
//! The engine boots one [`LaneAgent`](crate::lane_agent::LaneAgent) for each
//! of the grid, the wireframe and the gizmos, after the renderer in the
//! OUTPUT phase. Each lends its lane the frame's target and the host's View,
//! and commits its own pass slot; the FrameGraph composites them after the
//! UI, grid first and gizmos last.

mod agent;
