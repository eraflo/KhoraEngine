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

//! A game never attaches a system (Ergon spec 10): a system's component and
//! its state are the engine's, held by the one entity it keeps for the
//! system, so `GameWorld::add_declared_component` refuses both by name — as
//! it refuses a behavior's state — while the behaviors and components of the
//! same module attach.

use std::path::PathBuf;

use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::{DeclaredComponentError, GameWorld};

use khora_io::script::declarations::{apply_declarations, declaration_channel, DeclaredComponents};
use khora_io::script::hot_reload::{load_all, reload_channel};

const MODULE: &str = "clock.erg";

/// A system beside a behavior and a component, in one module.
const CLOCK: &str = "component Loot { int value = 1; }
                     behavior Guard { int hp = 10; }
                     system Clock {
                         int ticks = 0;
                         void Update(float dt) { ticks += 1; }
                     }";

/// A fresh directory holding `scripts/clock.erg`.
fn scripts_with_clock() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "khora-sdk-system-components-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));
    let scripts = root.join("scripts");
    std::fs::create_dir_all(&scripts).expect("a scratch script directory");
    std::fs::write(scripts.join(MODULE), CLOCK).expect("the script is written");
    root
}

/// A game world knowing what `clock.erg` declares, as the engine registers
/// it from the initial load.
fn world_declaring_clock() -> GameWorld {
    let root = scripts_with_clock();
    let (pending, declarations) = (reload_channel(), declaration_channel());
    assert_eq!(
        load_all(&root.join("scripts"), &pending, &declarations),
        1,
        "`{MODULE}` compiles"
    );
    let mut world = GameWorld::new();
    let report = apply_declarations(
        world.inner_world_mut(),
        &mut DeclaredComponents::default(),
        &declarations.drain(),
    );
    let _ = std::fs::remove_dir_all(&root);
    for name in ["Clock", "Clock#state", "Guard", "Loot"] {
        assert!(
            report.registered.contains(&name.to_owned()),
            "`{name}` is registered: {report:?}"
        );
    }
    world
}

/// **A system's component and its state are refused by name**, while the
/// module's behavior and component attach.
#[test]
fn a_systems_component_and_state_are_not_a_games_to_attach() {
    let mut world = world_declaring_clock();
    let entity = world.spawn(Transform::identity());

    for name in ["Clock", "Clock#state"] {
        assert_eq!(
            world.add_declared_component(entity, name, &[]),
            Err(DeclaredComponentError::NotDeclared(name.to_owned())),
            "`{name}` is the engine's"
        );
    }
    world
        .add_declared_component(entity, "Guard", &[("hp", ScriptValue::Int(3))])
        .expect("the behavior attaches");
    world
        .add_declared_component(entity, "Loot", &[])
        .expect("the component attaches");
}
