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

//! Keeps the one entity each script system runs on, named `system:Name`
//! ([`PersistentId::named`]), while a script declares that system.
//!
//! See the book: concepts/scripting/behaviors-and-systems.md, "Its one entity".

use crate::ecs::SystemContext;
use khora_core::ecs::PersistentId;
use khora_core::lane::OutputDeck;
use khora_core::script::ScriptValue;
use khora_core::Runtime;

use crate::ecs::{ComponentKey, DataSystemRegistration, ScriptRole, TickPhase, World};

/// Returns the identity of a system's entity: `system:` and the system's name.
pub fn singleton_id(system: &str) -> PersistentId {
    PersistentId::named(&format!("system:{system}"))
}

fn script_systems(
    world: &mut World,
    _runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
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
        observes: &[],
        role: crate::ecs::Role::Serves(khora_core::script::SCRIPT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::Role;

    /// **Script systems serve the scripting family**, so the runner keeps them
    /// off while scripts are off.
    #[test]
    fn script_systems_serve_the_scripting_family() {
        let registration = inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == "script_systems")
            .expect("script_systems is registered");

        assert_eq!(registration.role, Role::Serves(khora_core::script::SCRIPT));
    }
}
