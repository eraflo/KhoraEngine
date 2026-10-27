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

//! Shadow rendering algorithm — the atlases, the depth passes, and the
//! device resources a level of the shadows lane builds.
//!
//! Every function takes its dimensions from the level's [`Tier`]: the lane
//! **is** its quality, no config struct is injected.

pub mod atlas_2d;
pub mod atlas_cube;
pub mod bindings;
pub mod pass;
pub mod state;

pub use state::{ShadowGpu, Tier};
