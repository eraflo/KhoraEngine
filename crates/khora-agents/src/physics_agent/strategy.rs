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

//! The simulation strategies the physics agent negotiates between.

/// Strategies for physics simulation.
///
/// Debug-overlay extraction was previously a third variant here, but it
/// is not a *strategy* of the same mission ("step the simulation") — it
/// is a side-channel projection of provider state into the `World`.
/// That work now lives in the
/// [`physics_debug_extraction`](khora_data::ecs::systems::physics_debug_extraction)
/// `DataSystem` so the simulation continues to step regardless of whether
/// the debug overlay is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PhysicsStrategy {
    /// Standard high-precision physics.
    #[default]
    Standard,
    /// Simplified physics for low-power mode.
    Simplified,
}
