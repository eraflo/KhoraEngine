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

//! `PhysicsStepResult` — what the physics lane reports after a step.

/// Slot type written into [`OutputDeck`](khora_core::lane::OutputDeck)
/// by `StandardPhysicsLane` after `provider.step(dt)` completes.
///
/// Drained in `Maintenance` by the `physics_world_writeback` DataSystem,
/// which uses its presence to decide whether the simulation actually
/// advanced this frame (and therefore whether to pull fresh transforms
/// out of the provider). When the agent skips its lane (paused agent,
/// budget exhaustion …) no `PhysicsStepResult` lands in the deck and the
/// writeback no-ops.
#[derive(Debug, Default, Clone, Copy)]
pub struct PhysicsStepResult {
    /// The simulation timestep that was advanced.
    pub dt: f32,
}
