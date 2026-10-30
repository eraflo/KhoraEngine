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

//! The UI family, registered and booted as the engine boots it: one
//! `LaneAgent` built from the registration the family submitted, checked
//! against the runtime before the first frame.
//!
//! These replace the UI agent's own tests: its id, its phase, its importance,
//! the one lane it held (by name and kind), the one strategy it offered, what
//! it declared — the atlas it locked is now read — and that it leaves the
//! asset store to the residency. A frame drawn through the agent into the
//! frame's target needs a `FrameContext`, which this crate cannot build (it
//! takes a tokio handle); the lane's frames are `khora-lanes/tests/it/ui/`.

pub mod fixtures;
mod frames;
mod glyphs_on_gpu;
mod registration;
mod text_layout;
