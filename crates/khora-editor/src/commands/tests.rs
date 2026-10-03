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

//! Play and Stop through the editor's own commands.

use khora_sdk::prelude::math::Vec3;
use khora_sdk::SceneFile;

use super::*;

/// What Stop must bring back of an entity: identity, name, translation,
/// parent's identity, whether it is lit.
type Observed = (
    khora_sdk::khora_core::ecs::PersistentId,
    Option<String>,
    Option<Vec3>,
    Option<khora_sdk::khora_core::ecs::PersistentId>,
    bool,
);

/// The authored scene as Stop must restore it, sorted by identity.
fn observed(world: &GameWorld) -> Vec<Observed> {
    let inner = world.inner_world();
    let mut seen: Vec<_> = world
        .iter_entities()
        .map(|e| {
            (
                inner.persistent_id(e).expect("every entity has an id"),
                world
                    .get_component::<Name>(e)
                    .map(|n| n.as_str().to_owned()),
                world.get_component::<Transform>(e).map(|t| t.translation),
                inner
                    .get::<Parent>(e)
                    .and_then(|p| inner.persistent_id(p.0)),
                world.get_component::<Light>(e).is_some(),
            )
        })
        .collect();
    seen.sort_by_key(|(id, ..)| *id);
    seen
}

/// Play snapshots the scene for the fastest load — the snapshot encoding,
/// read back by the same build at Stop — and Stop restores it exactly: the
/// same entities under the same identities, the same values, the hierarchy.
#[test]
fn play_stop_uses_the_snapshot() {
    let mut world = GameWorld::new();
    let root = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        GlobalTransform::identity(),
        Name::new("Root"),
    ));
    let lamp = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)),
        GlobalTransform::identity(),
        Name::new("Lamp"),
        Light::directional(),
    ));
    world.inner_world_mut().set_parent(lamp, Some(root));
    for entity in [root, lamp] {
        world
            .inner_world_mut()
            .mark_authored(entity)
            .expect("authored");
    }
    let before = observed(&world);
    let state = Arc::new(Mutex::new(EditorState::default()));

    apply_play(&mut world, &state);
    let snapshot = state
        .lock()
        .expect("the editor state")
        .scene_snapshot
        .clone()
        .expect("Play keeps a snapshot");
    let file = SceneFile::from_bytes(&snapshot).expect("the snapshot is a scene file");
    let encoding = String::from_utf8_lossy(&file.header.encoding_id)
        .trim_end_matches('\0')
        .to_owned();
    assert_eq!(encoding, "KH_SNAPSHOT_V1", "Play snapshots for FastestLoad");

    // Play rearranges the world; Stop brings the authored one back.
    world.despawn(lamp);
    world.spawn((Transform::identity(), Name::new("Spawned in play")));
    apply_stop(&mut world, &state);
    assert_eq!(observed(&world), before, "Stop restores the scene exactly");
    assert_eq!(
        state.lock().expect("the editor state").play_mode,
        PlayMode::Editing
    );
}
