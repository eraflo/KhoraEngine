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

//! What the substrate hands a [`Flow`](super::Flow) beside the world.

use crate::ecs::ComponentKey;

/// What the substrate hands a Flow's [`select`](super::Flow::select) beside
/// the world: what it needs to know of its own runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlowContext {
    /// Whether this run starts the Flow over on this world: its first run on
    /// it, or its first run after the substrate skipped it (it serves a
    /// family that was off, [`Flow::ROLE`](super::Flow::ROLE)). `false` on a
    /// run that follows its previous run with no skip between.
    pub resumed: bool,
    /// The marker the Flow's family is limited to, when the family is scoped
    /// (`khora_core::agent::State::Scoped`): the Flow projects only the
    /// entities holding it. `None` for a family on unscoped, and for an
    /// [`Upkeep`](crate::ecs::Role::Upkeep) Flow. A marker name the world
    /// does not know is the key a component declared under that name would
    /// have, [`ComponentKey::named`], which no entity holds.
    pub scope: Option<ComponentKey>,
}
