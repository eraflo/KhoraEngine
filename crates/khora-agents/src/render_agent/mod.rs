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

//! Acts as the **[A]gent** for the rendering subsystem, fulfilling the role of an ISA.
//!
//! This module provides the high-level, tactical logic for rendering scenes. It
//! determines *which* rendering strategy to use and in what order, but delegates
//! the actual GPU command generation to the various `render_lanes`.
//!
//! As an **Intelligent Subsystem Agent (ISA)**, its responsibilities will include:
//! - Analyzing the scene's complexity (e.g., light count, geometry density) and
//!   reporting performance metrics (GPU time, VRAM usage) to the DCC.
//! - Managing a portfolio of rendering strategies, such as `SimpleUnlitLane`,
//!   `ForwardPlusLane`, or a future `DeferredLane`.
//! - Selecting the optimal rendering strategy based on the budget allocated by GORNA
//!   and the current performance goals (e.g., prioritizing framerate vs. visual quality).

mod mesh_preparation;

pub use mesh_preparation::*;
