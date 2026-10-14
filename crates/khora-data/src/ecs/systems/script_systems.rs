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

//! The one entity each script system runs on.
//!
//! A `system` is a behavior with no subject: it runs as an instance of its
//! component on an entity of its own, which nothing authors — named
//! `system:Name` ([`PersistentId::named`]) so a game save keeps it and a load
//! finds it again rather than making a second. It exists while gameplay runs:
//! spawned holding the system's component at its defaults (the script lane
//! gives it its state companion as it gives any behavior's), and despawned
//! once the name is no longer a system a script declares. Its components are
//! runtime state, so no scene ever holds it; the editor's Stop, which puts
//! back the world Play started from, takes it away.

use khora_core::agent::SharedEngineMode;
use khora_core::ecs::PersistentId;
use khora_core::lane::OutputDeck;
use khora_core::script::ScriptValue;
use khora_core::Runtime;

use crate::ecs::{ComponentKey, DataSystemRegistration, ScriptRole, TickPhase, World};

/// What a system's entity is named: `system:` and the system's name.
pub fn singleton_id(system: &str) -> PersistentId {
    PersistentId::named(&format!("system:{system}"))
}

fn script_systems(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let playing = runtime
        .resources
        .get::<SharedEngineMode>()
        .and_then(|shared| shared.read().ok().map(|mode| mode.runs_gameplay()))
        .unwrap_or(true);
    if !playing {
        return;
    }
    // Every name, and whether it is a system a script declares now: anything
    // else — a behavior, a component, a system no script declares — keeps no
    // entity.
    let named: Vec<(ComponentKey, String, bool)> = world
        .components()
        .iter()
        .map(|vtable| {
            let system = matches!(vtable.script, Some(ScriptRole::System { .. }))
                && vtable.declared_by.is_some();
            (vtable.key, vtable.name.to_string(), system)
        })
        .collect();
    for (key, name, system) in named {
        let id = singleton_id(&name);
        let held = world
            .entity_with_id(id)
            .filter(|entity| world.contains(*entity));
        match (held, system) {
            (Some(entity), false) => {
                world.despawn(entity);
            }
            (Some(entity), true) => {
                // An entity a restore brought back with nothing on it.
                if world.row(entity, key).is_none() {
                    attach(world, entity, key, &name);
                }
            }
            (None, true) => {
                let entity = world.spawn(());
                world.set_persistent_id(entity, id);
                attach(world, entity, key, &name);
            }
            (None, false) => {}
        }
    }
}

/// Gives `entity` system `name`'s component at its defaults.
fn attach(
    world: &mut World,
    entity: khora_core::ecs::entity::EntityId,
    key: ComponentKey,
    name: &str,
) {
    if let Err(error) = world.add_runtime_component(entity, key, &ScriptValue::Unit) {
        log::error!("the entity of system `{name}` cannot hold it: {error:?}");
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "script_systems",
        phase: TickPhase::PreExtract,
        // After the declarations are registered: a system declared this frame
        // has its entity before the script flow looks for what to run.
        order_hint: -8,
        run: script_systems,
        runs_after: &["script_components"],
    }
}
