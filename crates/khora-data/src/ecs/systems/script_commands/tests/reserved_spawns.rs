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

//! A spawn at an id reserved in the turn, as the frame boundary applies it.
//!
//! Every frame the `script_commands` system settles the reservations, applies
//! the commands in order — a `Spawn` commits its reserved id when every
//! component is accepted, and releases it otherwise — and then releases every
//! id handed out and never committed. That last step runs whatever the deck
//! holds, a frame with no command included: no reservation outlives its frame.

use std::collections::HashSet;

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{CommandBuffer, ScriptValue, WorldCommand};
use khora_core::Runtime;

use super::super::apply_script_commands;
use crate::ecs::{Name, Transform, World};

/// One frame of the `script_commands` system, its deck holding `commands`.
fn frame(world: &mut World, commands: Vec<WorldCommand>) {
    let mut deck = OutputDeck::new();
    deck.slot::<CommandBuffer>().extend(commands);
    apply_script_commands(
        world,
        &Runtime::default(),
        &mut deck,
        crate::ecs::SystemContext {
            since: crate::ecs::Tick(0),
        },
    );
}

/// One frame of the `script_commands` system with no command buffer at all —
/// a frame in which the script lane did not run.
fn frame_without_scripts(world: &mut World) {
    apply_script_commands(
        world,
        &Runtime::default(),
        &mut OutputDeck::new(),
        crate::ecs::SystemContext {
            since: crate::ecs::Tick(0),
        },
    );
}

/// A spawn at `entity`, at `position`, of `components`.
fn spawn(entity: EntityId, position: Vec3, components: Vec<(&str, ScriptValue)>) -> WorldCommand {
    WorldCommand::Spawn {
        entity,
        position,
        rotation: Quaternion::IDENTITY,
        components: components
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
    }
}

fn named(text: &str) -> Vec<(&'static str, ScriptValue)> {
    vec![("Name", ScriptValue::Str(text.to_owned()))]
}

fn name_of(world: &World, entity: EntityId) -> Option<&str> {
    world.get::<Name>(entity).map(|name| name.0.as_str())
}

/// Whether any live entity is named `text`.
fn anyone_named(world: &World, text: &str) -> bool {
    world
        .iter_entities()
        .any(|entity| name_of(world, entity) == Some(text))
}

/// Takes ids until `index` comes back, over a few frames — reserved and
/// committed, and spawned directly, each entity named "Reuse" so a write
/// through a stale id would have a component to land on — and returns every
/// id found at it.
fn next_uses_of(world: &mut World, index: u32) -> Vec<EntityId> {
    let mut found = Vec::new();
    for _ in 0..4 {
        let reserver = world.reserver();
        let reserved: Vec<EntityId> = (0..256).map(|_| reserver.reserve()).collect();
        let spawns = reserved
            .iter()
            .map(|&id| spawn(id, Vec3::ZERO, named("Reuse")))
            .collect();
        frame(world, spawns);
        let direct: Vec<EntityId> = (0..256)
            .map(|_| world.spawn((Transform::identity(), Name("Reuse".to_owned()))))
            .collect();
        found.extend(
            reserved
                .into_iter()
                .chain(direct)
                .filter(|id| id.index == index),
        );
        if !found.is_empty() {
            break;
        }
    }
    found
}

/// **The id the turn held is the entity the frame made.** A spawn at a
/// reserved id lives at exactly that id, placed where it was asked, carrying
/// what it was given.
#[test]
fn a_reserved_spawn_lives_at_exactly_its_reserved_id() {
    let mut world = World::new();
    let before = world.entity_count();
    let entity = world.reserver().reserve();

    frame(
        &mut world,
        vec![spawn(entity, Vec3::new(4.0, 5.0, 6.0), named("Explosion"))],
    );

    assert!(world.contains(entity), "{entity:?} is alive");
    assert_eq!(
        world.get::<Transform>(entity).map(|t| t.translation),
        Some(Vec3::new(4.0, 5.0, 6.0))
    );
    assert_eq!(name_of(&world, entity), Some("Explosion"));
    assert_eq!(world.entity_count(), before + 1, "one entity, no other");
}

/// **A command after the spawn configures it.** Queued in the same turn, a
/// `Set` naming the reserved id lands on the entity the spawn made.
#[test]
fn a_command_after_a_spawn_reaches_the_spawned_entity() {
    let mut world = World::new();
    let entity = world.reserver().reserve();

    frame(
        &mut world,
        vec![
            spawn(entity, Vec3::ZERO, named("Explosion")),
            WorldCommand::SetComponent {
                entity,
                component: "Name".into(),
                value: ScriptValue::Str("Crater".to_owned()),
            },
        ],
    );

    assert_eq!(name_of(&world, entity), Some("Crater"));
}

/// **No reservation outlives its frame** — a frame in which no script ran
/// included. The id is released, a spawn naming it later is refused, and its
/// index comes back with a newer generation.
#[test]
fn an_unspent_reservation_is_released_even_by_a_frame_without_scripts() {
    let mut world = World::new();
    world.spawn(Transform::identity());
    let unspent = world.reserver().reserve();

    frame_without_scripts(&mut world);
    let count = world.entity_count();

    frame(&mut world, vec![spawn(unspent, Vec3::ZERO, named("Late"))]);
    assert!(!world.contains(unspent), "a released id is never committed");
    assert!(!anyone_named(&world, "Late"), "the late spawn made nothing");
    assert_eq!(world.entity_count(), count);

    let reused = next_uses_of(&mut world, unspent.index);
    assert!(!reused.is_empty(), "the index was handed out again");
    assert!(
        reused.iter().all(|id| id.generation > unspent.generation),
        "{reused:?} reuse {unspent:?} without a newer generation"
    );
}

/// A frame whose deck holds an empty command buffer releases the same.
#[test]
fn an_unspent_reservation_is_released_by_a_frame_of_no_commands() {
    let mut world = World::new();
    let unspent = world.reserver().reserve();

    frame(&mut world, Vec::new());
    frame(&mut world, vec![spawn(unspent, Vec3::ZERO, named("Late"))]);

    assert!(!world.contains(unspent));
    assert!(!anyone_named(&world, "Late"));
}

/// **Thousands in a turn commit**: past whatever ids the reserver had ready,
/// it counts fresh ones, and every one of them comes alive at its id.
#[test]
fn reservations_past_the_ready_ids_all_commit() {
    const SPAWNS: usize = 3000;
    let mut world = World::new();
    let reserver = world.reserver();
    let reserved: Vec<EntityId> = (0..SPAWNS).map(|_| reserver.reserve()).collect();

    frame(
        &mut world,
        reserved
            .iter()
            .map(|&id| spawn(id, Vec3::ZERO, Vec::new()))
            .collect(),
    );

    assert_eq!(world.entity_count(), SPAWNS);
    assert!(reserved.iter().all(|id| world.contains(*id)));
    let live: HashSet<EntityId> = world.iter_entities().collect();
    assert_eq!(live, reserved.iter().copied().collect::<HashSet<_>>());
}

/// **A refused spawn never existed.** One component refuses its value: no
/// entity is made — not even with the components that were accepted — the
/// id is released, a second spawn naming it is refused, and its index comes
/// back with a newer generation.
#[test]
fn a_refused_spawn_leaves_no_entity_and_releases_its_id() {
    let mut world = World::new();
    world.spawn(Transform::identity());
    let count = world.entity_count();
    let refused = world.reserver().reserve();
    let mut components = named("Explosion");
    components.push(("RigidBody", ScriptValue::Str("heavy".to_owned())));

    frame(
        &mut world,
        vec![
            spawn(refused, Vec3::ZERO, components),
            spawn(refused, Vec3::ZERO, named("Second")),
        ],
    );

    assert!(!world.contains(refused), "a refused spawn left an entity");
    assert!(!anyone_named(&world, "Explosion"), "half a spawn was left");
    assert!(
        !anyone_named(&world, "Second"),
        "a released id was committed"
    );
    assert_eq!(world.entity_count(), count);

    let reused = next_uses_of(&mut world, refused.index);
    assert!(!reused.is_empty(), "the index was handed out again");
    assert!(
        reused.iter().all(|id| id.generation > refused.generation),
        "{reused:?} reuse {refused:?} without a newer generation"
    );
}

/// An id is committed once: a second spawn at an id already alive is
/// refused, and the first entity keeps what it was given.
#[test]
fn a_second_spawn_at_a_committed_id_is_refused() {
    let mut world = World::new();
    let entity = world.reserver().reserve();

    frame(
        &mut world,
        vec![
            spawn(entity, Vec3::ZERO, named("First")),
            spawn(entity, Vec3::ONE, named("Second")),
        ],
    );

    assert_eq!(world.entity_count(), 1);
    assert_eq!(name_of(&world, entity), Some("First"));
    assert!(!anyone_named(&world, "Second"));
}

/// **A despawned spawn never names another entity.** Committed, then
/// despawned, its index comes back with a newer generation, and a write
/// through the old id reaches nothing.
#[test]
fn a_spawned_then_despawned_id_never_names_another_entity() {
    let mut world = World::new();
    let entity = world.reserver().reserve();
    frame(&mut world, vec![spawn(entity, Vec3::ZERO, named("Bullet"))]);
    assert!(world.contains(entity));
    frame(&mut world, vec![WorldCommand::Despawn { entity }]);
    assert!(!world.contains(entity));

    let reused = next_uses_of(&mut world, entity.index);
    assert!(!reused.is_empty(), "the index was handed out again");
    assert!(reused.iter().all(|id| id.generation > entity.generation));

    frame(
        &mut world,
        vec![WorldCommand::SetComponent {
            entity,
            component: "Name".into(),
            value: ScriptValue::Str("Ghost".to_owned()),
        }],
    );
    assert!(
        !anyone_named(&world, "Ghost"),
        "the old id reached an entity"
    );
}

/// **A reservation is its world's.** An id another world reserved names
/// nothing this world handed out, so a spawn at it is refused — however
/// alike the two worlds' histories, and however many ids this world has
/// ready.
#[test]
fn a_reservation_from_another_world_is_refused() {
    let alike = || {
        let mut world = World::new();
        let spawned: Vec<EntityId> = (0..3).map(|_| world.spawn(Transform::identity())).collect();
        frame(
            &mut world,
            vec![WorldCommand::Despawn { entity: spawned[1] }],
        );
        world
    };
    let other = alike();
    let mut world = alike();
    let count = world.entity_count();
    let foreign = other.reserver().reserve();

    frame(
        &mut world,
        vec![spawn(foreign, Vec3::ZERO, named("Stranger"))],
    );

    assert!(!world.contains(foreign), "a foreign id was committed");
    assert!(!anyone_named(&world, "Stranger"));
    assert_eq!(world.entity_count(), count);

    let own = world.reserver().reserve();
    frame(&mut world, vec![spawn(own, Vec3::ZERO, named("Native"))]);
    assert!(
        world.contains(own),
        "the world's own reservation still commits"
    );
}

/// **A command naming a refused spawn's entity reaches nothing.** A spawn at
/// an id this world never handed out is refused, and what the turn queued
/// after it for that entity — which it believes it just made — must not land
/// on the live entity that happens to hold the same index and generation.
#[test]
fn a_write_to_a_refused_foreign_spawn_never_lands_on_a_live_entity() {
    let mut world = World::new();
    let original = world.spawn((Transform::identity(), Name("Original".to_owned())));
    // A reserver no world owns — a host run without the world's — hands out
    // the very id the world's first entity holds.
    let foreign = khora_core::ecs::EntityReserver::new().reserve();
    assert_eq!(foreign, original, "the ids alias");

    frame(
        &mut world,
        vec![
            spawn(foreign, Vec3::ZERO, named("Stranger")),
            WorldCommand::SetComponent {
                entity: foreign,
                component: "Name".into(),
                value: ScriptValue::Str("Hijacked".to_owned()),
            },
        ],
    );

    assert!(
        !anyone_named(&world, "Stranger"),
        "the foreign spawn was refused"
    );
    assert_eq!(
        name_of(&world, original),
        Some("Original"),
        "a write meant for the refused spawn landed on a live entity"
    );
}
