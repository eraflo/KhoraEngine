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

//! Shadow rendering algorithm — free functions shared by every quality
//! tier (`StandardShadowsLane`, `LowResShadowsLane`, future variants).
//!
//! Each function takes its dimensions as parameters; the lane that calls
//! it provides its own (hardcoded-in-the-type) constants. No config
//! struct is injected by the agent: the lane **is** its quality.

pub mod atlas_2d;
pub mod atlas_cube;
pub mod bindings;
pub mod pass;
pub mod state;

pub use state::ShadowsLaneState;
