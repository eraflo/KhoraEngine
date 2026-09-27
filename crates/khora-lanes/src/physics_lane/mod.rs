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

//! Physics Lane.
//!
//! [`StandardPhysicsLane`] is a thin wrapper around
//! [`PhysicsProvider::step`](khora_core::physics::PhysicsProvider::step): it does **no** World queries and
//! **no** structural mutations. The surrounding work is split into three
//! Substrate-Pass stages:
//!
//! 1. [`khora_data::flow::PhysicsFlow::adapt`] — AGDF detach/reattach
//!    plus the `ECS → provider` sync (`sync_to_world`-equivalent) that
//!    feeds the simulation its inputs.
//! 2. **This lane** — `provider.step(dt)`. Pure simulation, no World
//!    access at all.
//! 3. [`khora_data::ecs::systems::physics_world_writeback`] — pulls the
//!    new transforms, kinematic results, and collision events back into
//!    the World during the `Maintenance` phase.

mod standard;

pub use standard::StandardPhysicsLane;
