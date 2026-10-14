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

//! **A system's entity in a scene, whatever the encoding** (Ergon spec 10):
//! the goal a scene is saved for picks how it is written, never what it
//! holds — so no encoding of a scene saved while playing brings back a
//! system's entity.

use khora_core::scene::SerializationGoal;
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::Project;
use super::system_session::{singleton, SystemSession};

const CLOCK: Project = &[(
    "game/clock.erg",
    "system Clock { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

/// A scene of a world playing `CLOCK`, saved for `goal` and loaded into a
/// fresh world.
fn saved_while_playing(goal: SerializationGoal) -> World {
    let service = SerializationService::new();
    let mut session = SystemSession::new(World::new(), CLOCK);
    let authored = session.world.spawn(Transform::identity());
    session.world.mark_authored(authored).expect("authored");
    session.frame(0.1);
    assert!(singleton(&session.world, "Clock").is_some(), "its entity");
    let scene = service
        .save_world(&session.world, goal)
        .expect("the scene saves");
    let mut loaded = World::new();
    service
        .load_world(&scene, &mut loaded)
        .expect("the scene loads");
    loaded
}

/// **No encoding brings a system's entity back**: the same world saved for
/// each goal loads with no entity named for the system.
#[test]
fn no_scene_encoding_holds_a_systems_entity() {
    for goal in [
        SerializationGoal::EditorInterchange,
        SerializationGoal::HumanReadableDebug,
        SerializationGoal::PortableBinary,
        SerializationGoal::FastestLoad,
    ] {
        let loaded = saved_while_playing(goal);
        assert_eq!(
            singleton(&loaded, "Clock"),
            None,
            "a scene saved for {goal:?} holds the system's entity"
        );
    }
}
