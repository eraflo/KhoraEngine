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

//! The `loading` family: scenes, scripts and assets loaded off the frame.
//!
//! Hosts file a [`SceneLoadRequest`], the hot-reload pump a
//! [`CompileRequest`], the asset service a [`DecodeRequest`]; the family's
//! lanes, at `ExecutionPhase::BACKGROUND`, take them and stage each result
//! under its lease for the landings at a frame's boundary. Its two lanes are
//! two ways of loading the same requests: [`RushLane`] loads everything
//! filed, [`PacedLane`] only what its budget covers.
//!
//! [`SceneLoadRequest`]: khora_data::scene::loading::SceneLoadRequest
//! [`CompileRequest`]: khora_script::toolchain::CompileRequest
//! [`DecodeRequest`]: khora_core::asset::DecodeRequest

mod loading_family;
mod loading_inputs;
mod paced_lane;
mod rush_lane;
mod work;

pub use loading_family::*;
pub use loading_inputs::*;
pub use paced_lane::*;
pub use rush_lane::*;
