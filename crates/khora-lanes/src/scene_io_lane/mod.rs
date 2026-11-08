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

//! The `scene_io` family: scene loads prepared off the frame.
//!
//! A host files a [`SceneLoadRequest`]; the family's lane, at
//! `ExecutionPhase::BACKGROUND`, takes them, prepares each through the
//! runtime's [`SceneLoading`], and stages each [`PreparedLoad`] under its
//! lease, for the scene intake to commit at a frame's boundary.
//!
//! [`SceneLoadRequest`]: khora_data::scene::loading::SceneLoadRequest
//! [`SceneLoading`]: khora_data::scene::loading::SceneLoading
//! [`PreparedLoad`]: khora_data::scene::loading::PreparedLoad

mod scene_io;
mod scene_io_family;

pub use scene_io::*;
pub use scene_io_family::*;
