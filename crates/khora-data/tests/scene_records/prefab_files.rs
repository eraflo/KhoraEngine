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

//! Records that link to prefabs, written in every encoding and read back —
//! and a compact file written before records could link to prefabs.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_data::ecs::Transform;
use khora_data::scene::{
    instantiate_prefab, read_scene_file, serialize_prefab, write_scene_file, InstanceRecord,
    RemovedComponents, SaveRecord,
};

use super::prefab_sample::*;
use super::*;

/// A one-page record of `rows`, each holding a `Name`-like string.
fn named(rows: &[(PersistentId, &str)]) -> SceneRecord {
    SceneRecord {
        entities: rows.iter().map(|(id, _)| *id).collect(),
        pages: vec![PageRecord {
            components: vec!["Name".into()],
            rows: rows.iter().map(|(id, _)| *id).collect(),
            columns: vec![rows
                .iter()
                .map(|(_, name)| Record::Str((*name).into()))
                .collect()],
        }],
        instances: Vec::new(),
    }
}

/// A scene with a plain entity and two instances of two prefabs: one with an
/// override, a removed component and a deleted member; one left alone. Its
/// values are strings, which every encoding reads back as they were.
fn linking_scene() -> SceneRecord {
    let plain = PersistentId::authored(0x10);
    let (first, second) = (PersistentId::authored(0x20), PersistentId::authored(0x30));
    let (first_prefab, second_prefab) = (AssetUUID::new(), AssetUUID::new());
    let (barrel, sight) = (PersistentId::authored(0x21), PersistentId::authored(0x22));
    let mut scene = named(&[(plain, "Level")]);
    scene.instances = vec![
        InstanceRecord {
            root: first,
            prefab: first_prefab,
            delta: SaveRecord {
                base: first_prefab,
                order: vec![first, barrel],
                destroyed: vec![sight],
                created: Vec::new(),
                removed: vec![RemovedComponents {
                    entity: first,
                    components: vec!["Light".into()],
                }],
                changes: named(&[(barrel, "Gun")]),
                before: named(&[(barrel, "Barrel")]),
            },
        },
        InstanceRecord {
            root: second,
            prefab: second_prefab,
            delta: SaveRecord {
                base: second_prefab,
                order: vec![second],
                destroyed: Vec::new(),
                created: Vec::new(),
                removed: Vec::new(),
                changes: SceneRecord::default(),
                before: SceneRecord::default(),
            },
        },
    ];
    scene
}

/// Every encoding keeps a record's prefab links: each instance's root, its
/// prefab, and its delta — the order, the deleted members, the removed
/// components, the overrides and the prefab's values beside them. Through
/// the scene file too, header and all.
#[test]
fn every_encoding_keeps_a_records_prefab_links() {
    let scene = linking_scene();
    for (name, encoding) in every_encoding() {
        let back = through(&scene, name, encoding);
        assert_eq!(back.instances, scene.instances, "{name}: the links");
        assert_eq!(back, scene, "{name}: the record");

        let file = write_scene_file(&scene, encoding)
            .unwrap_or_else(|e| panic!("{name}: the file does not write: {e}"));
        let file = khora_core::scene::SceneFile::from_bytes(&file.to_bytes())
            .unwrap_or_else(|e| panic!("{name}: the file does not read: {e:?}"));
        let read = read_scene_file(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(read, scene, "{name}: through a file");
    }
}

/// A real scene with an instance — captured, collapsed — opens the same
/// from every encoding as from the record itself.
#[test]
fn a_scene_with_an_instance_opens_the_same_from_every_encoding() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    let barrel = entity(&world, PersistentId::within(instance, turret.barrel));
    world
        .get_mut::<Transform>(barrel)
        .expect("placed")
        .translation
        .x = 3.0;
    let scene = save(&world, &library);
    assert_eq!(scene.instances.len(), 1);

    for (name, encoding) in every_encoding() {
        let back = through(&scene, name, encoding);
        assert_eq!(back.instances.len(), 1, "{name}: the link is kept");
        let loaded = open(&back, &library);
        assert_same_world(&world, &loaded, name);
        let barrel = entity(&loaded, PersistentId::within(instance, turret.barrel));
        assert_eq!(
            translation(&loaded, barrel),
            Vec3::new(3.0, 1.0, 0.0),
            "{name}"
        );
    }
}

/// A compact file of layout 1 — written before records linked to prefabs —
/// still reads, as a record with no instance; today's writer marks its files
/// with layout 2, the layout that carries the instances.
#[test]
fn a_compact_file_from_before_prefab_links_still_reads() {
    let id = PersistentId::authored(0x0102_0304_0506_0708);
    let mut layout_1 = vec![
        1, // layout version
        1, 4, b'N', b'a', b'm', b'e', // symbols: "Name"
        0,    // no shape
        1,    // one entity
    ];
    layout_1.extend_from_slice(&id.to_bits().to_le_bytes());
    layout_1.extend_from_slice(&[
        1, // one page
        1, 0, // one component: symbol 0, "Name"
        1, // one row
    ]);
    layout_1.extend_from_slice(&id.to_bits().to_le_bytes());
    layout_1.extend_from_slice(&[8, 3, b'O', b'l', b'd']); // a string, "Old"

    let expected = named(&[(id, "Old")]);
    assert_eq!(
        CompactEncoding
            .decode(&layout_1)
            .expect("a layout-1 file reads"),
        expected
    );

    let written = CompactEncoding.encode(&linking_scene()).expect("encodes");
    assert_eq!(written.first(), Some(&2), "today's compact layout is 2");
}

/// Only a scene or a prefab may link to prefabs: the differences a game save
/// or an instance holds may not — read back, either is refused by every
/// encoding. That is what keeps a file from nesting links inside links
/// without end, each level a deeper call when it is read.
#[test]
fn differences_that_link_to_prefabs_are_refused_by_every_encoding() {
    let linking = linking_scene();
    let save_linking = SaveRecord {
        base: AssetUUID::new(),
        order: linking.entities.clone(),
        destroyed: Vec::new(),
        created: linking.entities.clone(),
        removed: Vec::new(),
        changes: linking.clone(),
        before: SceneRecord::default(),
    };
    let mut before_linking = save_linking.clone();
    before_linking.changes = SceneRecord::default();
    before_linking.before = linking.clone();
    let mut scene = named(&[(PersistentId::authored(0x40), "Level")]);
    scene.instances = vec![InstanceRecord {
        root: PersistentId::authored(0x50),
        prefab: AssetUUID::new(),
        delta: save_linking.clone(),
    }];

    for (name, encoding) in every_encoding() {
        for (what, save) in [("changes", &save_linking), ("before", &before_linking)] {
            if let Ok(bytes) = encoding.encode_save(save) {
                assert!(
                    encoding.decode_save(&bytes).is_err(),
                    "{name}: a save whose {what} link to prefabs reads"
                );
            }
        }
        if let Ok(bytes) = encoding.encode(&scene) {
            assert!(
                encoding.decode(&bytes).is_err(),
                "{name}: an instance whose differences link to prefabs reads"
            );
        }
    }
}

/// A tower instance saved over the file of the turret it holds — the tower
/// prefab contains the turret, so the turret file would contain itself
/// through it — is written whole. The turret file then spawns the tower's
/// content; the tower prefab, which links to the turret, still spawns; and
/// a scene holding the tower saves and opens again.
#[test]
fn a_prefab_written_with_an_instance_that_contains_it_stays_loadable() {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let tower_prefab = AssetUUID::new();
    let library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    let library = library.with(tower_prefab, tower.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, tower_prefab, &library).expect("instantiates");

    let bytes = serialize_prefab(&world, root, turret_prefab, &library).expect("writes");
    let file = khora_core::scene::SceneFile::from_bytes(&bytes).expect("a scene file");
    let written = read_scene_file(&file).expect("reads");
    assert!(written.instances.is_empty(), "{:?}", written.instances);
    let library = library.with(turret_prefab, written);

    let mut spawned = World::new();
    instantiate_prefab(&mut spawned, turret_prefab, &library).expect("the turret file spawns");
    assert_eq!(
        spawned.iter_entities().count(),
        4,
        "the tower's four entities"
    );
    let mut towers = World::new();
    instantiate_prefab(&mut towers, tower_prefab, &library).expect("the tower spawns");
    assert_eq!(towers.iter_entities().count(), 1 + 4);
    let scene = save(&towers, &library);
    let loaded = open(&scene, &library);
    assert_same_world(&towers, &loaded, "tower over turret");
}
