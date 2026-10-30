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

//! A prefab spawned in play, as the frame boundary applies it.
//!
//! The `script_commands` system instantiates the prefab — read through the
//! [`PrefabLibrary`] the runtime holds — as a linked instance whose root sits
//! at the id the turn reserved. The root is placed where the spawn says,
//! keeping the prefab's rotation unless the spawn gives one, and the prefab's
//! scale; each component the spawn wrote merges onto the root's, or is added
//! when the root has none. Anything refused takes the whole instance with
//! it: nothing of it remains, and the id names nothing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::lane::OutputDeck;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{CommandBuffer, ScriptValue, WorldCommand};
use khora_core::Runtime;

use super::super::apply_script_commands;
use crate::ecs::{Name, PrefabInstance, RigidBody, Transform, World};
use crate::scene::{capture_subtree, PrefabLibrary, PrefabSource, SceneRecord};

/// Prefabs held in memory, by asset id.
#[derive(Debug, Clone, Default)]
struct Library(HashMap<AssetUUID, SceneRecord>);

impl PrefabSource for Library {
    fn prefab(&self, id: AssetUUID) -> Result<SceneRecord, String> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("no prefab {id} in the library"))
    }
}

/// The bullet prefab's root rotation.
fn prefab_rotation() -> Quaternion {
    Quaternion::new(0.0, 0.6, 0.0, 0.8)
}

/// The bullet prefab's root scale.
const PREFAB_SCALE: Vec3 = Vec3::new(2.0, 2.0, 2.0);

/// A bullet prefab: a root — placed far off, turned, scaled, with a heavy
/// `RigidBody` and no `Name` — and a child named "Trail". Its record and the
/// trail's id in the prefab.
fn bullet_prefab() -> (SceneRecord, PersistentId) {
    let mut world = World::new();
    let root = world.spawn((
        Transform::new(Vec3::new(9.0, 9.0, 9.0), prefab_rotation(), PREFAB_SCALE),
        RigidBody {
            mass: 5.0,
            ccd_enabled: true,
            ..RigidBody::default()
        },
    ));
    let trail = world.spawn((Transform::identity(), Name("Trail".to_owned())));
    world.set_parent(trail, Some(root));
    world.mark_authored(root).expect("authored");
    let trail_id = world.mark_authored(trail).expect("authored");
    let record = capture_subtree(&world, root).expect("the bullet captures");
    (record, trail_id)
}

/// A runtime whose library holds the bullet as `prefab`.
fn with_bullet(prefab: AssetUUID) -> (Runtime, PersistentId) {
    let (record, trail) = bullet_prefab();
    let mut runtime = Runtime::default();
    runtime
        .resources
        .insert(PrefabLibrary(Arc::new(Library(HashMap::from([(
            prefab, record,
        )])))));
    (runtime, trail)
}

/// One frame of the `script_commands` system over `runtime`, its deck
/// holding `commands`.
fn frame(world: &mut World, runtime: &Runtime, commands: Vec<WorldCommand>) {
    let mut deck = OutputDeck::new();
    deck.slot::<CommandBuffer>().extend(commands);
    apply_script_commands(
        world,
        runtime,
        &mut deck,
        crate::ecs::SystemContext {
            since: crate::ecs::Tick(0),
        },
    );
}

/// A prefab spawn of `prefab` at `entity` and `position`, writing
/// `components`.
fn spawn_prefab(
    entity: EntityId,
    prefab: AssetUUID,
    position: Vec3,
    rotation: Option<Quaternion>,
    components: Vec<(&str, ScriptValue)>,
) -> WorldCommand {
    WorldCommand::SpawnPrefab {
        entity,
        prefab,
        position,
        rotation,
        components: components
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
    }
}

fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

fn name_of(world: &World, entity: EntityId) -> Option<&str> {
    world.get::<Name>(entity).map(|name| name.0.as_str())
}

fn anyone_named(world: &World, text: &str) -> bool {
    world
        .iter_entities()
        .any(|entity| name_of(world, entity) == Some(text))
}

/// The world's live entities, and how many.
fn census(world: &World) -> (usize, HashSet<EntityId>) {
    (world.entity_count(), world.iter_entities().collect())
}

/// Whether any live entity carries a prefab link.
fn any_instance(world: &World) -> bool {
    world
        .iter_entities()
        .any(|entity| world.get::<PrefabInstance>(entity).is_some())
}

/// **The instance lands at the reserved id, linked.** Its root carries the
/// link and a created identity; the prefab's child is there under the id
/// derived from it, parented to the root.
#[test]
fn a_prefab_spawn_lands_a_linked_instance_at_the_reserved_id() {
    let prefab = AssetUUID::new();
    let (runtime, trail) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(root, prefab, Vec3::ZERO, None, Vec::new())],
    );

    assert!(world.contains(root), "the root lives at {root:?}");
    assert_eq!(
        world.get::<PrefabInstance>(root),
        Some(&PrefabInstance { prefab })
    );
    let instance = world.persistent_id(root).expect("identified");
    assert!(instance.is_created(), "{instance:?} is created");
    let member = world
        .entity_with_id(PersistentId::within(instance, trail))
        .expect("the trail is a member of the instance");
    assert_eq!(name_of(&world, member), Some("Trail"));
    assert_eq!(
        world.get::<crate::ecs::Parent>(member).map(|p| p.0),
        Some(root)
    );
    assert_eq!(world.entity_count(), 2, "the prefab's two entities");
}

/// **The root stands where the spawn says**, keeping the prefab's rotation
/// and scale when the spawn gives no rotation.
#[test]
fn the_root_is_placed_keeping_the_prefabs_rotation_and_scale() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(
            root,
            prefab,
            Vec3::new(1.0, 2.0, 3.0),
            None,
            Vec::new(),
        )],
    );

    let placed = world.get::<Transform>(root).expect("placed");
    assert_eq!(placed.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(placed.rotation, prefab_rotation(), "the prefab's rotation");
    assert_eq!(placed.scale, PREFAB_SCALE, "the prefab's scale");
}

/// **A rotation the spawn gives replaces the prefab's**; the scale stays the
/// prefab's.
#[test]
fn a_rotation_the_spawn_gives_replaces_the_prefabs() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();
    let facing = Quaternion::new(0.0, 0.0, 1.0, 0.0);

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(
            root,
            prefab,
            Vec3::new(4.0, 5.0, 6.0),
            Some(facing),
            Vec::new(),
        )],
    );

    let placed = world.get::<Transform>(root).expect("placed");
    assert_eq!(placed.translation, Vec3::new(4.0, 5.0, 6.0));
    assert_eq!(placed.rotation, facing);
    assert_eq!(placed.scale, PREFAB_SCALE);
}

/// **A component the spawn writes merges onto the root's**: the field it
/// names changes, the others keep the prefab's values.
#[test]
fn a_component_the_spawn_writes_merges_onto_the_roots() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(
            root,
            prefab,
            Vec3::ZERO,
            None,
            vec![("RigidBody", fields(&[("mass", ScriptValue::Float(2.0))]))],
        )],
    );

    let body = world.get::<RigidBody>(root).expect("the prefab's body");
    assert_eq!(body.mass, 2.0, "the field the spawn wrote");
    assert!(
        body.ccd_enabled,
        "a field the spawn left keeps the prefab's"
    );
}

/// **A component the root lacks is added**, with what the spawn wrote.
#[test]
fn a_component_the_root_lacks_is_added() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(
            root,
            prefab,
            Vec3::ZERO,
            None,
            vec![("Name", ScriptValue::Str("Shot".to_owned()))],
        )],
    );

    assert_eq!(name_of(&world, root), Some("Shot"));
    assert_eq!(
        world.get::<RigidBody>(root).map(|body| body.mass),
        Some(5.0),
        "the prefab's own components stay"
    );
}

/// **A command after the spawn reaches the root**, in the same batch.
#[test]
fn a_command_after_a_prefab_spawn_reaches_its_root() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![
            spawn_prefab(root, prefab, Vec3::ZERO, None, Vec::new()),
            WorldCommand::SetComponent {
                entity: root,
                component: "RigidBody".into(),
                value: fields(&[("mass", ScriptValue::Float(7.0))]),
            },
        ],
    );

    assert_eq!(
        world.get::<RigidBody>(root).map(|body| body.mass),
        Some(7.0)
    );
}

/// **A component refused takes the whole instance with it**: neither the
/// root nor the prefab's child remains, and the id names nothing.
#[test]
fn a_refused_component_leaves_nothing_of_the_instance() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    world.spawn((Transform::identity(), Name("Level".to_owned())));
    let before = census(&world);

    for components in [
        vec![("Wobble", ScriptValue::Int(1))],
        vec![
            ("Name", ScriptValue::Str("Shot".to_owned())),
            ("RigidBody", ScriptValue::Str("heavy".to_owned())),
        ],
    ] {
        let root = world.reserver().reserve();
        frame(
            &mut world,
            &runtime,
            vec![spawn_prefab(root, prefab, Vec3::ZERO, None, components)],
        );

        assert!(!world.contains(root), "the root of a refused spawn lives");
        assert!(!anyone_named(&world, "Trail"), "the prefab's child remains");
        assert!(!anyone_named(&world, "Shot"), "half a spawn remains");
        assert!(!any_instance(&world), "a link remains");
        assert_eq!(census(&world), before);
    }
}

/// **Without a library, every prefab spawn is refused**, and the world is
/// left as it was.
#[test]
fn without_a_prefab_library_a_prefab_spawn_is_refused() {
    let prefab = AssetUUID::new();
    let mut world = World::new();
    world.spawn((Transform::identity(), Name("Level".to_owned())));
    let before = census(&world);
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &Runtime::default(),
        vec![spawn_prefab(root, prefab, Vec3::ZERO, None, Vec::new())],
    );

    assert!(!world.contains(root));
    assert!(!any_instance(&world));
    assert_eq!(census(&world), before);
}

/// **A prefab the library does not hold is refused**, the world as it was.
#[test]
fn a_missing_prefab_is_refused() {
    let (runtime, _) = with_bullet(AssetUUID::new());
    let mut world = World::new();
    world.spawn((Transform::identity(), Name("Level".to_owned())));
    let before = census(&world);
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(
            root,
            AssetUUID::new(),
            Vec3::ZERO,
            None,
            Vec::new(),
        )],
    );

    assert!(!world.contains(root));
    assert!(!any_instance(&world));
    assert_eq!(census(&world), before);
}

/// **A refused prefab spawn never existed**: its id is released at the
/// frame's end, so a spawn naming it the next frame — the library there now
/// — is refused too.
#[test]
fn a_refused_prefab_spawn_releases_its_id() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &Runtime::default(),
        vec![spawn_prefab(root, prefab, Vec3::ZERO, None, Vec::new())],
    );
    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(root, prefab, Vec3::ZERO, None, Vec::new())],
    );

    assert!(!world.contains(root), "a released id was committed");
    assert_eq!(world.entity_count(), 0);
}

/// **A command naming a refused prefab spawn's root reaches nothing** — not
/// the live entity that happens to hold the same id.
#[test]
fn a_write_to_a_refused_prefab_spawn_never_lands_on_a_live_entity() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    let original = world.spawn((Transform::identity(), Name("Original".to_owned())));
    // A reserver no world owns hands out the very id the world's first
    // entity holds.
    let foreign = khora_core::ecs::EntityReserver::new().reserve();
    assert_eq!(foreign, original, "the ids alias");

    frame(
        &mut world,
        &runtime,
        vec![
            spawn_prefab(foreign, prefab, Vec3::ZERO, None, Vec::new()),
            WorldCommand::SetComponent {
                entity: foreign,
                component: "Name".into(),
                value: ScriptValue::Str("Hijacked".to_owned()),
            },
        ],
    );

    assert!(!any_instance(&world), "the foreign spawn was refused");
    assert_eq!(
        name_of(&world, original),
        Some("Original"),
        "a write meant for the refused spawn landed on a live entity"
    );
    assert_eq!(world.entity_count(), 1);
}

/// **A despawn after the spawn, in the same batch, takes the whole
/// instance**: the root and the prefab's child.
#[test]
fn a_despawn_after_a_prefab_spawn_takes_the_whole_instance() {
    let prefab = AssetUUID::new();
    let (runtime, _) = with_bullet(prefab);
    let mut world = World::new();
    world.spawn((Transform::identity(), Name("Level".to_owned())));
    let before = census(&world);
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![
            spawn_prefab(root, prefab, Vec3::ZERO, None, Vec::new()),
            WorldCommand::Despawn { entity: root },
        ],
    );

    assert!(!world.contains(root));
    assert!(!anyone_named(&world, "Trail"), "the prefab's child remains");
    assert_eq!(census(&world), before);
}

/// **A prefab whose root has no `Transform` is placed all the same**: it is
/// given one where the spawn says, turned as the spawn says or not at all,
/// at scale one.
#[test]
fn a_prefab_root_without_a_transform_is_given_one() {
    let prefab = AssetUUID::new();
    let mut source = World::new();
    let root_in_prefab = source.spawn(Name("Marker".to_owned()));
    source.mark_authored(root_in_prefab).expect("authored");
    let record = capture_subtree(&source, root_in_prefab).expect("the marker captures");
    let mut runtime = Runtime::default();
    runtime
        .resources
        .insert(PrefabLibrary(Arc::new(Library(HashMap::from([(
            prefab, record,
        )])))));
    let mut world = World::new();
    let facing = Quaternion::new(0.0, 0.0, 1.0, 0.0);
    let (upright, turned) = (world.reserver().reserve(), world.reserver().reserve());

    frame(
        &mut world,
        &runtime,
        vec![
            spawn_prefab(upright, prefab, Vec3::new(1.0, 2.0, 3.0), None, Vec::new()),
            spawn_prefab(
                turned,
                prefab,
                Vec3::new(4.0, 5.0, 6.0),
                Some(facing),
                Vec::new(),
            ),
        ],
    );

    let placed = world.get::<Transform>(upright).expect("given a transform");
    assert_eq!(placed.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(placed.rotation, Quaternion::IDENTITY);
    assert_eq!(placed.scale, Vec3::ONE);
    let placed = world.get::<Transform>(turned).expect("given a transform");
    assert_eq!(placed.translation, Vec3::new(4.0, 5.0, 6.0));
    assert_eq!(placed.rotation, facing);
}

/// **A refused component takes every entity of the prefab with it** — one
/// the prefab holds beside its root, not under it, too.
#[test]
fn a_refused_component_leaves_nothing_of_a_prefab_with_two_top_entities() {
    let prefab = AssetUUID::new();
    let mut source = World::new();
    let first = source.spawn((Transform::identity(), Name("Front".to_owned())));
    let second = source.spawn((Transform::identity(), Name("Beside".to_owned())));
    source.mark_authored(first).expect("authored");
    source.mark_authored(second).expect("authored");
    let record = crate::scene::capture_world(&source).expect("the pair captures");
    let mut runtime = Runtime::default();
    runtime
        .resources
        .insert(PrefabLibrary(Arc::new(Library(HashMap::from([(
            prefab, record,
        )])))));
    let mut world = World::new();
    let before = census(&world);
    let root = world.reserver().reserve();

    frame(
        &mut world,
        &runtime,
        vec![spawn_prefab(
            root,
            prefab,
            Vec3::ZERO,
            None,
            vec![("Wobble", ScriptValue::Int(1))],
        )],
    );

    assert!(!world.contains(root));
    assert!(!anyone_named(&world, "Front"), "the root remains");
    assert!(
        !anyone_named(&world, "Beside"),
        "the entity beside the root remains"
    );
    assert_eq!(census(&world), before);
}
