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

//! Entity references inside components, across a save.

use khora_core::script::{FrozenValue, ScriptValue};
use khora_data::ecs::{Children, Name, Parent, Script, Transform, World};
use khora_data::scene::record::ReportKind;

use super::sample::suspended_snapshot;
use super::*;

/// The entity `world` knows by the identity `src_entity` had in `src`.
fn twin(src: &World, dst: &World, src_entity: EntityId) -> EntityId {
    let id = src
        .persistent_id(src_entity)
        .expect("a live entity has an id");
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// The entity a script field holds.
fn field_entity(script: &Script, field: &str) -> EntityId {
    match script.field(field) {
        Some(ScriptValue::Entity(entity)) => *entity,
        other => panic!("field `{field}` holds {other:?}, not an entity"),
    }
}

/// The entity the frozen machine of `script` holds in its registers.
fn frozen_entity(script: &Script) -> EntityId {
    let pending = script.runtime.pending.as_ref().expect("a pending sequence");
    pending
        .machine
        .registers
        .iter()
        .find_map(|register| match register {
            FrozenValue::Entity(entity) => Some(*entity),
            _ => None,
        })
        .expect("a register holds an entity")
}

/// A world where one entity refers to three others every way a component can:
/// a parent, a script field, a list of entities in a script field, and an
/// entity in a register of a frozen sequence.
fn referring_world() -> (World, [EntityId; 4]) {
    let mut world = World::new();
    // Ids a fresh world would not hand out in this order.
    let scratch: Vec<_> = (0..3).map(|_| world.spawn(Transform::identity())).collect();
    for entity in scratch {
        world.despawn(entity);
    }
    let a = world.spawn((Transform::identity(), Name::new("A")));
    let b = world.spawn((Transform::identity(), Name::new("B")));
    let c = world.spawn((Transform::identity(), Name::new("C")));
    let holder = world.spawn((
        Transform::identity(),
        Name::new("Holder"),
        Script {
            module: "ai/holder.erg".into(),
            behavior: "Holder".into(),
            fields: vec![
                ("target".into(), ScriptValue::Entity(b)),
                (
                    "crowd".into(),
                    ScriptValue::Array(vec![ScriptValue::Entity(c), ScriptValue::Entity(a)]),
                ),
            ],
            runtime: suspended_snapshot(c),
        },
    ));
    world.set_parent(holder, Some(a));
    (world, [a, b, c, holder])
}

/// A reference is to an entity, not to the slot it happened to occupy: after a
/// load, every reference — `Parent`, a script field, a list of entities, a
/// register of a frozen sequence — names the reloaded entity it named before.
#[test]
fn a_reference_inside_a_component_is_remapped() {
    let (src, [a, b, c, holder]) = referring_world();

    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(applied.report.is_clean(), "{name}: {:?}", applied.report);
        let holder_back = twin(&src, &dst, holder);

        let parent = dst.get::<Parent>(holder_back).expect("the parent survives");
        assert_eq!(parent.0, twin(&src, &dst, a), "{name}: Parent");

        let script = dst.get::<Script>(holder_back).expect("the script survives");
        assert_eq!(
            field_entity(script, "target"),
            twin(&src, &dst, b),
            "{name}: a script field"
        );
        assert_eq!(
            script.field("crowd"),
            Some(&ScriptValue::Array(vec![
                ScriptValue::Entity(twin(&src, &dst, c)),
                ScriptValue::Entity(twin(&src, &dst, a)),
            ])),
            "{name}: a list of entities"
        );
        assert_eq!(
            frozen_entity(script),
            twin(&src, &dst, c),
            "{name}: a register of a frozen sequence"
        );
    }
}

/// `Children` is not saved — it is the inverse of `Parent` — so the load
/// rebuilds it, in every encoding, for every level of the tree.
#[test]
fn hierarchy_survives_every_encoding() {
    let mut src = World::new();
    let root = src.spawn((Transform::identity(), Name::new("Root")));
    let left = src.spawn((Transform::identity(), Name::new("Left")));
    let right = src.spawn((Transform::identity(), Name::new("Right")));
    let leaf = src.spawn((Transform::identity(), Name::new("Leaf")));
    src.set_parent(left, Some(root));
    src.set_parent(right, Some(root));
    src.set_parent(leaf, Some(left));

    for (name, encoding) in every_encoding() {
        let (dst, _) = reload(&src, name, encoding);
        let [root_back, left_back, right_back, leaf_back] =
            [root, left, right, leaf].map(|e| twin(&src, &dst, e));

        assert!(
            dst.get::<Parent>(root_back).is_none(),
            "{name}: the root has no parent"
        );
        assert_eq!(
            dst.get::<Parent>(left_back).map(|p| p.0),
            Some(root_back),
            "{name}"
        );
        assert_eq!(
            dst.get::<Parent>(right_back).map(|p| p.0),
            Some(root_back),
            "{name}"
        );
        assert_eq!(
            dst.get::<Parent>(leaf_back).map(|p| p.0),
            Some(left_back),
            "{name}"
        );

        let mut under_root = dst
            .get::<Children>(root_back)
            .unwrap_or_else(|| panic!("{name}: the root's children were not rebuilt"))
            .0
            .clone();
        under_root.sort_by_key(|e| (e.index, e.generation));
        let mut expected = vec![left_back, right_back];
        expected.sort_by_key(|e| (e.index, e.generation));
        assert_eq!(under_root, expected, "{name}: children of the root");
        assert_eq!(
            dst.get::<Children>(left_back).map(|c| c.0.clone()),
            Some(vec![leaf_back]),
            "{name}: children of the left branch"
        );
        assert!(
            dst.get::<Children>(leaf_back)
                .is_none_or(|c| c.0.is_empty()),
            "{name}: a leaf has no children"
        );
        assert!(dst.is_descendant_of(leaf_back, root_back), "{name}");
    }
}

/// A record of one holder whose script fields name two entities the record
/// does not hold, and the holder's identity.
fn dangling_record() -> (PersistentId, SceneRecord) {
    let mut src = World::new();
    let gone_one = src.spawn((Transform::identity(), Name::new("GoneOne")));
    let gone_two = src.spawn((Transform::identity(), Name::new("GoneTwo")));
    let holder = src.spawn((
        Transform::identity(),
        Name::new("Holder"),
        Script {
            module: "ai/holder.erg".into(),
            behavior: "Holder".into(),
            fields: vec![
                ("first".into(), ScriptValue::Entity(gone_one)),
                ("second".into(), ScriptValue::Entity(gone_two)),
                ("first_again".into(), ScriptValue::Entity(gone_one)),
            ],
            runtime: Default::default(),
        },
    ));
    let holder_id = src.persistent_id(holder).expect("id");
    let gone_ids = [gone_one, gone_two].map(|e| src.persistent_id(e).expect("id"));

    // The two targets are recorded nowhere: their references dangle.
    let mut record = capture_world(&src).expect("captures");
    without_entities(&mut record, &gone_ids);
    assert_eq!(record.entities, vec![holder_id]);
    assert!(
        record.pages.iter().all(|page| page.rows == vec![holder_id]),
        "only the holder keeps rows: {:?}",
        record.pages
    );
    (holder_id, record)
}

/// The `first`, `second` and `first_again` fields of the holder `applied`
/// brought in.
fn dangling_fields(world: &World, applied: &Applied, name: &str) -> [EntityId; 3] {
    let &(_, holder) = applied
        .entities
        .first()
        .unwrap_or_else(|| panic!("{name}: the holder loaded"));
    let script = world
        .get::<Script>(holder)
        .unwrap_or_else(|| panic!("{name}: its script loaded"));
    ["first", "second", "first_again"].map(|field| field_entity(script, field))
}

/// A reference to an entity the save does not hold loads as the world's one
/// **nowhere** entity: every such reference — to the same missing entity or
/// to different ones — names the same id, which is not alive and never will
/// be (no later spawn is handed it), and each is reported. A second load into
/// the same world names that same entity again: no load reserves one of its
/// own.
#[test]
fn a_dangling_reference_names_the_one_nowhere_entity() {
    let (holder_id, record) = dangling_record();

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: a dangling reference is no error: {e}"));
        let holder_back = dst.entity_with_id(holder_id).expect("the holder loaded");
        let [first, second, first_again] = dangling_fields(&dst, &applied, name);
        assert_eq!(first, first_again, "{name}: one missing entity, one id");
        assert_eq!(
            first, second,
            "{name}: two missing entities still name the one nowhere entity"
        );
        let nowhere = first;
        assert!(!dst.contains(nowhere), "{name}: nowhere is not alive");
        assert_ne!(nowhere, holder_back, "{name}");
        assert_eq!(
            dst.persistent_id(nowhere),
            None,
            "{name}: nowhere has no identity"
        );

        let dead: Vec<_> = applied
            .report
            .entries
            .iter()
            .filter(|entry| entry.kind == ReportKind::DeadReference)
            .collect();
        assert!(!dead.is_empty(), "{name}: dead references are reported");
        for entry in dead {
            assert_eq!(entry.entity, Some(holder_id), "{name}: {entry:?}");
            assert_eq!(
                entry.component.as_deref(),
                Some("Script"),
                "{name}: {entry:?}"
            );
        }

        for _ in 0..64 {
            let spawned = dst.spawn(Transform::identity());
            assert_ne!(
                spawned.index, nowhere.index,
                "{name}: a spawn was handed nowhere's slot"
            );
        }
        assert!(!dst.contains(nowhere), "{name}");

        // A second load beside the first: the same entity, not a new one.
        let again = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: the second load failed: {e}"));
        assert_eq!(
            dangling_fields(&dst, &again, name),
            [nowhere; 3],
            "{name}: a later load reuses the world's nowhere entity"
        );
        assert!(!dst.contains(nowhere), "{name}");
    }
}

/// Both kinds of unbindable reference — to an identity the save lacks, and
/// one a subtree wrote as outside it — name the same entity, across loads of
/// different records into one world.
#[test]
fn every_unbindable_reference_in_a_world_names_the_same_entity() {
    // A subtree whose root's script names an entity outside it.
    let mut src = World::new();
    let outsider = src.spawn((Transform::identity(), Name::new("Outsider")));
    let root = src.spawn((
        Transform::identity(),
        Name::new("Root"),
        Script {
            module: "ai/root.erg".into(),
            behavior: "Root".into(),
            fields: vec![("enemy".into(), ScriptValue::Entity(outsider))],
            runtime: Default::default(),
        },
    ));
    let subtree = khora_data::scene::capture_subtree(&src, root).expect("captures");
    let (_, dangling) = dangling_record();

    for (name, encoding) in every_encoding() {
        let subtree = through(&subtree, name, encoding);
        let dangling = through(&dangling, name, encoding);
        let mut dst = World::new();
        let mut named = Vec::new();
        for _ in 0..2 {
            let applied = apply(&mut dst, &subtree, Identity::Fresh)
                .unwrap_or_else(|e| panic!("{name}: the subtree loads: {e}"));
            let &(_, root_back) = applied.entities.first().expect("the root loaded");
            let script = dst.get::<Script>(root_back).expect("its script loaded");
            named.push(field_entity(script, "enemy"));
            let applied = apply(&mut dst, &dangling, Identity::Keep)
                .unwrap_or_else(|e| panic!("{name}: the dangling record loads: {e}"));
            named.extend(dangling_fields(&dst, &applied, name));
        }
        let nowhere = named[0];
        assert!(
            named.iter().all(|&entity| entity == nowhere),
            "{name}: every unbindable reference names one entity: {named:?}"
        );
        assert!(!dst.contains(nowhere), "{name}: and it is not alive");
    }
}

/// However many loads meet a dead reference, the world reserves one entity
/// for all of them: load the same record and despawn what it brought a
/// hundred times, and every slot comes back but that one — the next spawn's
/// index stays small.
#[test]
fn many_loads_with_dead_references_do_not_grow_the_entity_store() {
    let (_, record) = dangling_record();
    let mut dst = World::new();
    let mut nowhere = None;
    for round in 0..100 {
        let applied = apply(&mut dst, &record, Identity::Keep)
            .unwrap_or_else(|e| panic!("round {round}: the load failed: {e}"));
        let [first, ..] = dangling_fields(&dst, &applied, "load");
        assert_eq!(
            *nowhere.get_or_insert(first),
            first,
            "round {round}: a load reserved an entity of its own"
        );
        for &(_, entity) in &applied.entities {
            assert!(dst.despawn(entity), "round {round}: the holder despawns");
        }
    }
    assert_eq!(dst.iter_entities().count(), 0);
    let next = dst.spawn(Transform::identity());
    assert!(
        next.index < 8,
        "a hundred loads left their dead references holding slots: {next:?}"
    );
}
