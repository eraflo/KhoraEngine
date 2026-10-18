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

//! The world's entity reserver, published for the lanes that spawn.
//!
//! A handle, not a copy: a script's `Spawn` takes its entity's id from it in
//! the turn, and the world commits or takes the id back at the frame's end.
//! Projecting it reads the world and changes nothing.

use std::sync::Arc;

use khora_core::ecs::EntityReserver;

use super::{Flow, Selection};
use crate::ecs::{SemanticDomain, World};
use crate::register_flow;
use khora_core::Runtime;

/// The reserver of the world a frame's spawns are for.
#[derive(Debug, Clone)]
pub struct ReservationView {
    /// Where a spawn takes its entity's id.
    pub reserver: Arc<EntityReserver>,
}

/// Publishes [`ReservationView`].
#[derive(Debug, Default)]
pub struct ReservationFlow;

impl Flow for ReservationFlow {
    type View = ReservationView;
    const DOMAIN: SemanticDomain = SemanticDomain::Script;
    const NAME: &'static str = "reservations";

    fn project(&self, world: &World, _sel: &Selection, _runtime: &Runtime) -> Self::View {
        ReservationView {
            reserver: world.reserver(),
        }
    }
}

register_flow!(ReservationFlow);
