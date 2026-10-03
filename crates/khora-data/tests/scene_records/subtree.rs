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

//! Part of a world, written down and brought back as a new instance.

use std::collections::HashSet;

use khora_core::script::ScriptValue;
use khora_data::ecs::{Children, Name, Parent, Script, Transform, World};
use khora_data::scene::record::{EntityRef, Record, VariantPayload};
use khora_data::scene::{capture_subtree, SaveError};

use super::*;

/// Every entity reference a record holds, wherever it sits.
pub(super) fn references_in(record: &Record, found: &mut Vec<EntityRef>) {
    match record {
        Record::Entity(reference) => found.push(*reference),
        Record::Some(inner) => references_in(inner, found),
        Record::Newtype { value, .. } => references_in(value, found),
        Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
            items.iter().for_each(|item| references_in(item, found))
        }
        Record::Map(entries) => entries.iter().for_each(|(key, value)| {
            references_in(key, found);
            references_in(value, found);
        }),
        Record::Struct { fields, .. } => fields
            .iter()
            .for_each(|(_, value)| references_in(value, found)),
        Record::Variant { payload, .. } => match payload {
            VariantPayload::Unit => {}
            VariantPayload::Newtype(inner) => references_in(inner, found),
            VariantPayload::Tuple(items) => items.iter().for_each(|i| references_in(i, found)),
            VariantPayload::Struct(fields) => fields
                .iter()
                .for_each(|(_, value)| references_in(value, found)),
        },
        _ => {}
    }
}

/// A prefab-shaped world: a level holding a turret (root), its barrel (child)
/// and its sight (grandchild); the turret's script aims at the sight — inside
/// the subtree — and remembers the player — outside it.
struct Level {
    world: World,
    player: EntityId,
    turret: EntityId,
    barrel: EntityId,
    sight: EntityId,
}

fn level() -> Level {
    let mut world = World::new();
    let level_root = world.spawn((Transform::identity(), Name::new("Level")));
    let player = world.spawn((Transform::identity(), Name::new("Player")));
    let turret = world.spawn((Transform::identity(), Name::new("Turret")));
    let barrel = world.spawn((Transform::identity(), Name::new("Barrel")));
    let sight = world.spawn((Transform::identity(), Name::new("Sight")));
    world.set_parent(turret, Some(level_root));
    world.set_parent(barrel, Some(turret));
    world.set_parent(sight, Some(barrel));
    world
        .add_component(
            turret,
            Script {
                module: "ai/turret.erg".into(),
                behavior: "Turret".into(),
                fields: vec![
                    ("aim".into(), ScriptValue::Entity(sight)),
                    ("enemy".into(), ScriptValue::Entity(player)),
                ],
            },
        )
        .expect("a script attaches");
    for entity in [level_root, player, turret, barrel, sight] {
        world.mark_authored(entity).expect("authored");
    }
    Level {
        world,
        player,
        turret,
        barrel,
        sight,
    }
}

/// A subtree records its root and everything under it — nothing else — and
/// a reference that leaves the subtree is written as outside it, not as an
/// identity the record does not hold. The root's own `Parent` is left out:
/// the instance hangs wherever it is made.
#[test]
fn a_subtree_capture_holds_only_the_subtree() {
    let level = level();
    let record = capture_subtree(&level.world, level.turret).expect("the subtree captures");

    let recorded: HashSet<_> = record.entities.iter().copied().collect();
    assert_eq!(
        recorded.len(),
        record.entities.len(),
        "an entity listed twice"
    );
    for page in &record.pages {
        for row in &page.rows {
            assert!(
                recorded.contains(row),
                "a row of {row:?}, which is not listed"
            );
        }
    }
    let turret = level.world.persistent_id(level.turret).expect("id");
    let barrel = level.world.persistent_id(level.barrel).expect("id");
    assert_eq!(
        slot_of(&record, turret, "Parent"),
        None,
        "the root's parent is outside the subtree"
    );
    assert!(
        slot_of(&record, barrel, "Parent").is_some(),
        "a parent inside the subtree is recorded"
    );
    let expected: HashSet<_> = [level.turret, level.barrel, level.sight]
        .map(|e| level.world.persistent_id(e).expect("id"))
        .into_iter()
        .collect();
    assert_eq!(recorded, expected);

    let mut references = Vec::new();
    for value in record
        .pages
        .iter()
        .flat_map(|page| page.columns.iter().flatten())
    {
        references_in(value, &mut references);
    }
    let player = level.world.persistent_id(level.player).expect("id");
    assert!(
        !references.contains(&EntityRef::Id(player)),
        "the player is outside the subtree"
    );
    assert!(references.contains(&EntityRef::Outside), "{references:?}");
    for reference in references {
        if let EntityRef::Id(id) = reference {
            assert!(
                recorded.contains(&id),
                "{id:?} is referenced but not recorded"
            );
        }
    }
}

/// A subtree whose root is gone cannot be captured.
#[test]
fn a_subtree_of_a_dead_entity_is_an_error() {
    let mut level = level();
    let turret = level.turret;
    level.world.despawn_subtree(turret);
    assert_eq!(
        capture_subtree(&level.world, turret),
        Err(SaveError::NoSuchEntity(turret))
    );
}

/// Instantiating a subtree twice gives two independent copies: each with new
/// authored identities, disjoint from the original's and from each other's;
/// each wired to itself — the child's parent, the script's aim — and not to the
/// original or the other copy; with a root that hangs from nothing.
#[test]
fn a_subtree_round_trips_as_a_fresh_instance() {
    let mut level = level();
    let record = capture_subtree(&level.world, level.turret).expect("captures");
    let original_ids: HashSet<_> = level
        .world
        .iter_entities()
        .filter_map(|e| level.world.persistent_id(e))
        .collect();

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut seen_ids = original_ids.clone();
        let mut instances = Vec::new();
        for copy in 0..2 {
            let before = level.world.iter_entities().count();
            let applied = apply(&mut level.world, &back, Identity::Fresh)
                .unwrap_or_else(|e| panic!("{name}: instance {copy} failed: {e}"));
            assert_eq!(level.world.iter_entities().count(), before + 3, "{name}");
            assert_eq!(applied.entities.len(), 3, "{name}");
            for (recorded, entity) in &applied.entities {
                let id = level
                    .world
                    .persistent_id(*entity)
                    .expect("an instance entity has an id");
                assert!(!id.is_created(), "{name}: an instance is authored");
                assert_ne!(id, *recorded, "{name}: a fresh instance takes new ids");
                assert!(seen_ids.insert(id), "{name}: {id:?} is not fresh");
            }
            instances.push(applied);
        }

        for applied in &instances {
            let new_of = |original: EntityId| {
                let recorded = level.world.persistent_id(original).expect("id");
                applied
                    .entities
                    .iter()
                    .find(|(id, _)| *id == recorded)
                    .map(|(_, entity)| *entity)
                    .unwrap_or_else(|| panic!("{name}: {recorded:?} was not instantiated"))
            };
            let (turret, barrel, sight) = (
                new_of(level.turret),
                new_of(level.barrel),
                new_of(level.sight),
            );

            assert!(
                level.world.get::<Parent>(turret).is_none(),
                "{name}: the instance root keeps no parent from the original"
            );
            assert_eq!(
                level.world.get::<Parent>(barrel).map(|p| p.0),
                Some(turret),
                "{name}"
            );
            assert_eq!(
                level.world.get::<Parent>(sight).map(|p| p.0),
                Some(barrel),
                "{name}"
            );
            assert_eq!(
                level.world.get::<Children>(turret).map(|c| c.0.clone()),
                Some(vec![barrel]),
                "{name}"
            );

            let script = level
                .world
                .get::<Script>(turret)
                .expect("the script came along");
            assert_eq!(
                script.field("aim"),
                Some(&ScriptValue::Entity(sight)),
                "{name}: a reference inside the subtree points inside the instance"
            );
            match script.field("enemy") {
                Some(ScriptValue::Entity(enemy)) => {
                    assert!(
                        !level.world.contains(*enemy),
                        "{name}: a reference outside the subtree is dead in the instance"
                    );
                }
                other => panic!("{name}: `enemy` holds {other:?}"),
            }
        }

        // The original is untouched by its copies.
        let script = level
            .world
            .get::<Script>(level.turret)
            .expect("original script");
        assert_eq!(script.field("aim"), Some(&ScriptValue::Entity(level.sight)));
        assert_eq!(
            level
                .world
                .get::<Children>(level.turret)
                .map(|c| c.0.clone()),
            Some(vec![level.barrel])
        );
    }
}
