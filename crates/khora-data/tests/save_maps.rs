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

//! Real string-keyed maps in scenes and game saves — a key a struct marker
//! would claim, a component the author adds after a save, a map nested in
//! a struct whose keys play removed — and a save older than a type rename.
//!
//! Its own test binary: the components declared here are registered for the
//! whole process, and the scene-record binary requires its sample world to
//! carry every registered component.

use std::collections::BTreeMap;

use khora_core::asset::AssetUUID;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::math::Vec3;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::{
    apply, capture_save, capture_world, compose, prepare_game, read_save_file, write_save_file,
    CompactEncoding, Identity, MsgPackEncoding, SaveRecord, SceneEncoding, SceneRecord,
    TextEncoding,
};
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The `crate::ecs` the derive expands against.
mod ecs {
    pub use khora_data::ecs::*;
}

/// The `crate::scene` the derive expands against.
mod scene {
    pub use khora_data::scene::*;
}

/// A component holding a real string-keyed map.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Spatial)]
pub struct Lexicon {
    pub words: BTreeMap<String, String>,
}

/// A struct holding a real map beside a plain field.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Entries {
    pub words: BTreeMap<String, u32>,
    pub count: u32,
}

/// A component whose real map sits inside a struct field.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Spatial)]
pub struct Glossary {
    pub entries: Entries,
    pub label: String,
}

fn every_encoding() -> [(&'static str, &'static dyn SceneEncoding); 3] {
    [
        ("compact", &CompactEncoding),
        ("text", &TextEncoding),
        ("msgpack", &MsgPackEncoding),
    ]
}

fn through(record: &SceneRecord, name: &str, encoding: &dyn SceneEncoding) -> SceneRecord {
    let bytes = encoding
        .encode(record)
        .unwrap_or_else(|e| panic!("{name}: encode failed: {e}"));
    encoding
        .decode(&bytes)
        .unwrap_or_else(|e| panic!("{name}: decode failed: {e}"))
}

fn save_through(save: &SaveRecord, name: &str, encoding: &dyn SceneEncoding) -> SaveRecord {
    let file = write_save_file(save, encoding).unwrap_or_else(|e| panic!("{name}: {e}"));
    read_save_file(&file).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn load(base: &SceneRecord, save: &SaveRecord) -> World {
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(base, save))
        .unwrap_or_else(|e| panic!("the game did not load: {e}"));
    prepared.commit(&mut dst, Identity::Keep);
    dst
}

fn twin(dst: &World, id: PersistentId) -> EntityId {
    dst.entity_with_id(id)
        .unwrap_or_else(|| panic!("nothing in the loaded world is {id:?}"))
}

/// **A component the designer added since the save.** The game changed `A`'s
/// translation and nothing else; the designer then gives `A` a `Lexicon`. The
/// game never saw that component, so it cannot have removed it.
#[test]
fn a_component_the_designer_added_since_the_save_reaches_a_diverged_entity() {
    let mut world = World::new();
    let a = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new("A"),
    ));
    let a_id = world.mark_authored(a).expect("authored");
    let base = capture_world(&world).expect("captures");
    world.get_mut::<Transform>(a).expect("placed").translation.y = 9.0;
    let save = capture_save(&world, AssetUUID::new(), &base).expect("saves");

    let mut editor = World::new();
    apply(&mut editor, &base, Identity::Keep).expect("the scene opens");
    editor
        .add_component(twin(&editor, a_id), Lexicon::default())
        .expect("attaches");
    let edited = capture_world(&editor).expect("captures");

    let dst = load(&edited, &save);
    let back = twin(&dst, a_id);
    assert_eq!(
        dst.get::<Transform>(back).map(|t| t.translation.y),
        Some(9.0)
    );
    assert!(
        dst.get::<Lexicon>(back).is_some(),
        "the designer's new component was dropped from an entity the game moved"
    );
}

/// **A `$struct` key in a real map.** A scene holding a map whose keys a user
/// typed — one of them `$struct` — reads back as that map, in every encoding.
#[test]
fn a_map_with_a_struct_key_reads_back_as_the_map() {
    let mut world = World::new();
    let mut words = BTreeMap::new();
    words.insert("$struct".to_owned(), "Hello".to_owned());
    words.insert("greeting".to_owned(), "Bonjour".to_owned());
    let lexicon = Lexicon { words };
    let entity = world.spawn(lexicon.clone());
    let entity_id = world.mark_authored(entity).expect("authored");
    let record = capture_world(&world).expect("captures");

    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        apply(&mut dst, &back, Identity::Keep).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            dst.get::<Lexicon>(twin(&dst, entity_id)),
            Some(&lexicon),
            "{name}: the map changed"
        );
    }
}

/// **A real map inside a struct, keys removed by play.** The game drops one
/// key and bumps the count beside it; through every save encoding and every
/// base encoding the map comes back with the key gone and the other field
/// kept from the patch.
#[test]
fn a_key_removed_from_a_nested_map_stays_removed_in_every_encoding() {
    let mut world = World::new();
    let mut words = BTreeMap::new();
    words.insert("one".to_owned(), 1);
    words.insert("two".to_owned(), 2);
    let entity = world.spawn(Glossary {
        entries: Entries { words, count: 2 },
        label: "authored".to_owned(),
    });
    let entity_id = world.mark_authored(entity).expect("authored");
    let base = capture_world(&world).expect("captures");

    {
        let glossary = world.get_mut::<Glossary>(entity).expect("there");
        glossary.entries.words.remove("two");
        glossary.entries.count = 1;
    }
    let expected = world.get::<Glossary>(entity).cloned();

    for (base_name, base_encoding) in every_encoding() {
        let read_base = through(&base, base_name, base_encoding);
        let save = capture_save(&world, AssetUUID::new(), &read_base).expect("saves");
        for (save_name, save_encoding) in every_encoding() {
            let read_save = save_through(&save, save_name, save_encoding);
            let dst = load(&read_base, &read_save);
            assert_eq!(
                dst.get::<Glossary>(twin(&dst, entity_id)).cloned(),
                expected,
                "base {base_name}, save {save_name}"
            );
        }
    }
}

/// A component whose type was renamed: older saves call it `LegacyWard`.
#[derive(Debug, Clone, PartialEq, Default, Component)]
#[component(domain = Spatial, formerly = "LegacyWard")]
pub struct Ward {
    pub radius: f32,
    pub power: f32,
}

/// Renames every `from` column of `record` to `to`, returning how many.
fn rename_component(record: &mut SceneRecord, from: &str, to: &str) -> usize {
    let mut renamed = 0;
    for name in record
        .pages
        .iter_mut()
        .flat_map(|page| page.components.iter_mut())
    {
        if name == from {
            *name = to.to_owned();
            renamed += 1;
        }
    }
    renamed
}

/// **A save older than a type rename, merged with a later scene edit.** The
/// save calls the ward `LegacyWard`, both as the game left it and as the
/// scene held it then. The game widened the radius; the author has since
/// raised the power, which the game left alone. Today's `Ward` carries both.
#[test]
fn a_save_older_than_a_type_rename_merges_with_a_later_scene_edit() {
    let mut world = World::new();
    let entity = world.spawn(Ward {
        radius: 1.0,
        power: 1.0,
    });
    let entity_id = world.mark_authored(entity).expect("authored");
    let base = capture_world(&world).expect("captures");
    world.get_mut::<Ward>(entity).expect("there").radius = 5.0;
    let mut save = capture_save(&world, AssetUUID::new(), &base).expect("saves");
    assert_eq!(rename_component(&mut save.changes, "Ward", "LegacyWard"), 1);
    assert_eq!(rename_component(&mut save.before, "Ward", "LegacyWard"), 1);

    let mut editor = World::new();
    apply(&mut editor, &base, Identity::Keep).expect("the scene opens");
    editor
        .get_mut::<Ward>(twin(&editor, entity_id))
        .expect("there")
        .power = 3.0;
    let edited = capture_world(&editor).expect("captures");

    for (name, encoding) in every_encoding() {
        let read_save = save_through(&save, name, encoding);
        let dst = load(&through(&edited, name, encoding), &read_save);
        assert_eq!(
            dst.get::<Ward>(twin(&dst, entity_id)),
            Some(&Ward {
                radius: 5.0,
                power: 3.0
            }),
            "{name}"
        );
    }
}
