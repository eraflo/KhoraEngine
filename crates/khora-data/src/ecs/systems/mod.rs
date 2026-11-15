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

//! The data layer's invariant systems: each module submits one
//! [`crate::ecs::DataSystemRegistration`], run in its [`crate::ecs::TickPhase`].
//!
//! See the book: concepts/the-frame.md, "The Substrate Pass".

pub mod asset_eviction;
pub mod audio_playback_writeback;
pub mod behavior_writeback;
pub mod capture_previous_transform;
pub mod collision_dispatch;
pub mod collision_to_script;
pub mod component_checks;
pub mod ecs_maintenance;
pub mod ensure_global_transform;
pub mod ibl_bake;
pub mod input_map_update;
pub mod physics_debug_extraction;
pub mod physics_release;
pub mod physics_world_writeback;
pub mod script_commands;
pub mod script_systems;
pub mod transform_propagation;

pub use transform_propagation::transform_propagation_system;
