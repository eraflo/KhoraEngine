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

//! Physics debug-render extraction — opt-in DataSystem.
//!
//! Lifts the previous `PhysicsDebugLane` (which was registered as a second
//! lane on `PhysicsAgent` and *replaced* the simulation when active — i.e.
//! enabling debug overlay also stopped physics) into a side-channel
//! [`crate::ecs::DataSystem`] that runs in `PostSimulation` *alongside* the
//! normal physics step. Per CLAD: debug data extraction is a Data-domain
//! invariant projection, not an alternative agent strategy.
//!
//! Activation is per-entity via [`PhysicsDebugData::enabled`]: when no
//! entity has the component, the system is a no-op.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::physics::PhysicsProvider;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, PhysicsDebugData, TickPhase, World};

fn physics_debug_extraction(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(provider_arc) = runtime
        .backends
        .get::<Arc<Mutex<Box<dyn PhysicsProvider>>>>()
    else {
        return;
    };

    // Cheap pre-check: skip the lock entirely when no entity carries
    // `PhysicsDebugData`. Avoids contending the physics provider mutex on
    // every frame in shipping builds.
    if world.query::<&PhysicsDebugData>().next().is_none() {
        return;
    }

    let guard = match provider_arc.lock() {
        Ok(g) => g,
        Err(e) => {
            log::error!("physics_debug_extraction: provider mutex poisoned: {}", e);
            return;
        }
    };

    for debug_data in world.query_mut::<&mut PhysicsDebugData>() {
        if debug_data.enabled {
            let (vertices, indices) = guard.get_debug_render_data();
            debug_data.vertices = vertices;
            debug_data.indices = indices;
        } else {
            debug_data.vertices.clear();
            debug_data.indices.clear();
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "physics_debug_extraction",
        phase: TickPhase::PostSimulation,
        run: physics_debug_extraction,
        order_hint: 100, // After transform_propagation (which runs at default 0).
        runs_after: &["transform_propagation"],
    }
}
