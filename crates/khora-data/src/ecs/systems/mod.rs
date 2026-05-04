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

//! Invariant systems of the Data layer (Substrate Pass).
//!
//! Each module here defines one [`crate::ecs::DataSystemRegistration`] that
//! gets dispatched by the engine in its declared [`crate::ecs::TickPhase`].
//! The dispatcher in `khora-control` discovers them automatically through
//! `inventory`; nothing else needs to know about a new system except its
//! own file.

pub mod ecs_maintenance;
pub mod gpu_mesh_sync;
pub mod transform_propagation;

pub use transform_propagation::transform_propagation_system;
