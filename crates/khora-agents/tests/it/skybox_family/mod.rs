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

//! The skybox family, registered and booted as the engine boots it: one
//! `LaneAgent` built from the registration the family submitted, checked
//! against the runtime before the first frame.
//!
//! These replace the skybox agent's own tests: its id, its phase after the
//! renderer, its importance, the one lane it held, the one strategy it
//! offered, and what it declared. A frame drawn through the agent into the
//! frame's target needs a `FrameContext`, which this crate cannot build (it
//! takes a tokio handle); those frames are the SDK's tests
//! (`khora-sdk/tests/it/frame_target_reaches_lanes.rs`).

pub mod fixtures;
mod frames;
mod registration;
