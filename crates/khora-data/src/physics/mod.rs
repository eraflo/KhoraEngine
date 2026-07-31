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

//! Physics-domain data types that are **not** ECS components.
//!
//! ECS components live in [`crate::ecs::components::physics`]. This
//! module hosts physics-domain types that flow through `Resources` or
//! lane outputs — values the engine shares between systems but that
//! have no per-entity identity.

pub mod collision;

pub use collision::{CollisionPair, CollisionPairs};
