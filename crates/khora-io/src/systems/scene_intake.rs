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

//! The scene loads prepared off the frame, committed at its boundary in the
//! order they were filed.

use std::sync::Arc;

use khora_core::lane::{OutputDeck, Reserve, Staged};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, SystemContext, TickPhase, World};
use khora_data::scene::loading::PreparedLoad;
use khora_data::scene::record::LoadReport;
use khora_data::scene::{Identity, LoadFailure};

use crate::scene_loads::{LoadOutcome, SceneLoadHub};
use crate::serialization::{SerializationService, SerializationServiceError};

/// Commits into `world` every prepared load the reserve holds, and files each outcome.
///
/// Publishes `world`'s catalogue to the runtime's `SceneLoadHub` first, when
/// it moved. Nothing without a hub or a reserve.
pub fn take_in_loads(world: &mut World, runtime: &Runtime) {
    let Some(hub) = runtime.resources.get::<Arc<SceneLoadHub>>() else {
        return;
    };
    hub.publish(world);
    let Some(reserve) = runtime.look_up::<Reserve>() else {
        return;
    };
    // In the order the loads were filed, whatever order they were prepared
    // in: one prepared again holds back those filed after it.
    let mut turn = hub.in_turn(reserve.take::<PreparedLoad>()).into_iter();
    while let Some(load) = turn.next() {
        match land(load, world) {
            Some((ticket, outcome)) => hub.land(ticket, outcome),
            None => {
                hub.wait_again(turn);
                break;
            }
        }
        // A load that registered what its file declares moved the
        // catalogue: the loads after it are checked against the new one.
        hub.publish(world);
    }
}

/// Commits `load` into `world`, and returns the ticket and the outcome to file.
///
/// `None` when it was filed again: prepared against components the world
/// changed since.
fn land(
    load: PreparedLoad,
    world: &mut World,
) -> Option<(khora_data::scene::loading::LoadTicket, LoadOutcome)> {
    let ticket = load.ticket;
    let (prepared, left_out) = match load.outcome {
        Ok(prepared) => prepared,
        Err(message) => {
            let failure = LoadFailure {
                message,
                report: LoadReport::default(),
            };
            return Some((ticket, Err(SerializationServiceError::LoadFailed(failure))));
        }
    };
    if prepared.is_for(world) && !prepared.is_current(world) {
        // Prepared again off the frame rather than on it, and never after a
        // replace emptied the world.
        PreparedLoad {
            ticket,
            replaces: load.replaces,
            outcome: Ok((prepared, left_out)),
            refile: load.refile,
        }
        .abandon();
        return None;
    }
    let mut report = if load.replaces && prepared.is_for(world) {
        SerializationService::replace_with(prepared, world)
    } else {
        // A load prepared for another world adds nothing and says so.
        prepared.commit(world, Identity::Keep).report
    };
    report.entries.extend(left_out);
    Some((ticket, Ok(report)))
}

fn scene_intake_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    take_in_loads(world, runtime);
}

// At the frame boundary, whatever families are on: an editor editing a
// scene opens one, and restores the scene a play changed.
inventory::submit! {
    DataSystemRegistration {
        name: "scene_intake",
        phase: TickPhase::Boundary,
        run: scene_intake_system,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}
