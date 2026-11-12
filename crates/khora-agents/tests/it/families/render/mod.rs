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

//! The render family, registered and booted as the engine boots it: one
//! `LaneAgent` built from the registration the family submitted, checked
//! against the runtime before the first frame.
//!
//! These replace the hand-written render agent's tests: its id, its phase,
//! its importance, the lanes it held, the strategies it offered and what it
//! declared. A frame drawn through the agent into the frame's target needs a
//! `FrameContext` among the runtime's resources (built with
//! `FrameContext::new()`); the lanes' own frames are khora-lanes' tests
//! (`tests/it/render/`).

pub mod fixtures;
mod frames;
mod registration;
