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

//! A module saved empty for a frame — an editor's truncate-then-write caught
//! between its two halves — declares nothing and compiles to a program with
//! no behavior. The behavior's component stays in the world, orphaned, and
//! its instances live on. When the module comes back, every instance keeps
//! its values **by name**, as any other reload carries them.
//!
//! Driven by hand through the two queues the hot-reload pump fills, so the
//! order of what reaches the engine is the test's, not a file watcher's.

use std::path::PathBuf;

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::Transform;
use khora_sdk::prelude::math::Vec3;
use khora_sdk::{
    AgentProvider, DccService, DeclaredComponentError, EngineApp, EngineCore, GameWorld,
    InputEvent, PhaseProvider, Runtime, WindowConfig,
};

use khora_io::script::declarations::{
    declaration_channel, PendingDeclarations, ScriptDeclarations,
};
use khora_io::script::hot_reload::{load_all, reload_channel, PendingReloads};

const MODULE: &str = "mover.erg";

const MOVER: &str = "behavior Mover {
                         float speed = 1.5;
                         int ticks = 0;
                         void Update(float dt) { ticks += 1; }
                     }";

/// `MOVER` with a field inserted **above** the others: every slot after it
/// shifts by one.
const MOVER_WITH_EXTRA_FIRST: &str = "behavior Mover {
                                          int extra = 7;
                                          float speed = 1.5;
                                          int ticks = 0;
                                          void Update(float dt) { ticks += 1; }
                                      }";

/// A game whose setup attaches `Mover` with `speed = 2.0`.
struct AttachingMover {
    attached: Option<(EntityId, Result<(), DeclaredComponentError>)>,
}

impl AgentProvider for AttachingMover {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for AttachingMover {}
impl EngineApp for AttachingMover {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        AttachingMover { attached: None }
    }
    fn setup(&mut self, world: &mut GameWorld, _runtime: &Runtime) {
        let entity = world.spawn(Transform::from_translation(Vec3::ZERO));
        let attached =
            world.add_declared_component(entity, "Mover", &[("speed", ScriptValue::Float(2.0))]);
        self.attached = Some((entity, attached));
    }
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A headless game with no file watcher: what reaches it is what the test
/// sends through `pending` and `declarations`.
struct Game {
    engine: EngineCore<AttachingMover>,
    entity: EntityId,
    scripts: PathBuf,
    pending: PendingReloads,
    declarations: PendingDeclarations,
}

impl Game {
    fn start() -> Self {
        let scripts = std::env::temp_dir().join(format!(
            "khora-sdk-reload-after-an-emptied-module-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&scripts).expect("a scratch script directory");
        std::fs::write(scripts.join(MODULE), MOVER).expect("the script is written");

        let (pending, declarations) = (reload_channel(), declaration_channel());
        let mut runtime = Runtime::new();
        runtime.resources.insert(pending.clone());
        runtime.resources.insert(declarations.clone());
        assert_eq!(
            load_all(&scripts, &pending, &declarations),
            1,
            "`{MODULE}` compiles"
        );

        let mut engine = EngineCore::<AttachingMover>::new();
        engine.bootstrap(AttachingMover::new(), runtime);
        let entity = match engine.app_mut().and_then(|app| app.attached.clone()) {
            Some((entity, Ok(()))) => entity,
            other => panic!("setup attaches `Mover`, got {other:?}"),
        };
        Game {
            engine,
            entity,
            scripts,
            pending,
            declarations,
        }
    }

    /// What the pump sends for `MODULE` saved as `source`: its program, and
    /// what it declares now — nothing, when it declares nothing.
    fn save(&self, source: &str) {
        std::fs::write(self.scripts.join(MODULE), source).expect("the script is saved");
        let declared = declaration_channel();
        assert_eq!(
            load_all(&self.scripts, &self.pending, &declared),
            1,
            "it compiles"
        );
        let mut sets = declared.drain();
        if sets.is_empty() {
            sets.push(ScriptDeclarations {
                module: MODULE.to_owned(),
                components: Vec::new(),
            });
        }
        for set in sets {
            self.declarations.send(set);
        }
    }

    fn field(&mut self, field: &str) -> Option<ScriptValue> {
        let world = self.engine.game_world_mut()?.inner_world();
        let key = world.components().key_named("Mover")?;
        world
            .declared_row(self.entity, key)?
            .into_iter()
            .find_map(|(name, value)| (name == field).then_some(value))
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scripts);
    }
}

/// **A reload carries an instance's values by name, even across a version of
/// its module that did not have the behavior.** Without it the instance keeps
/// its store laid out for the version before, and the field inserted above
/// `speed` shifts every value one slot down: `speed` takes what `ticks` held.
#[test]
fn a_field_inserted_after_an_emptied_save_keeps_every_value_by_name() {
    let mut game = Game::start();
    game.engine.tick();
    assert_eq!(
        game.field("ticks"),
        Some(ScriptValue::Int(1)),
        "`Mover` ran"
    );

    game.save("");
    game.engine.tick();
    game.save(MOVER_WITH_EXTRA_FIRST);
    game.engine.tick();

    assert_eq!(game.field("extra"), Some(ScriptValue::Int(7)));
    assert_eq!(
        game.field("speed"),
        Some(ScriptValue::Float(2.0)),
        "the value setup named"
    );
    assert!(
        matches!(game.field("ticks"), Some(ScriptValue::Int(n)) if n >= 2),
        "`Mover` runs on, counting from where it was: {:?}",
        game.field("ticks")
    );
}
