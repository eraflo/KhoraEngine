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

//! The three overlay families — grid, wireframe, gizmo — registered and
//! booted as the engine boots them: one `LaneAgent` each, built from the
//! registration its family submitted, checked against the runtime before the
//! first frame.
//!
//! These replace the overlay agent's own tests: its id, its three lanes by
//! name and kind, its phase after the renderer, the one strategy it offered
//! and what it declared. A frame drawn through an agent into the frame's
//! target needs a `FrameContext`, which this crate cannot build (it takes a
//! tokio handle); the lanes' frames are `khora-lanes/tests/it/overlays/`.

pub mod fixtures;
mod frames;
mod registration;
