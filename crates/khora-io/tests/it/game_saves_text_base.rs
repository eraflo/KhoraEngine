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

//! Game saves through the serialization service, against a scene kept in a
//! text or msgpack file — the formats that do not write struct names.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::scene::SerializationGoal;
use khora_data::ecs::{Name, Transform, World};
use khora_data::scene::{read_scene_file, write_scene_file, TextEncoding};
use khora_io::serialization::SerializationService;

/// **A designer edit reaches what the game left alone, whatever the scene's
/// encoding.** The game moves the guard; the designer later renames the
/// bystander, which the game never touched. Loading the save over the edited
/// scene must show the new name.
#[test]
fn a_designer_edit_reaches_an_untouched_entity_whatever_the_base_encoding() {
    let service = SerializationService::new();
    for goal in [
        SerializationGoal::EditorInterchange,
        SerializationGoal::LongTermStability,
        SerializationGoal::PortableBinary,
    ] {
        let mut world = World::new();
        let guard = world.spawn((Transform::identity(), Name::new("Guard")));
        let bystander = world.spawn((Transform::identity(), Name::new("Bystander")));
        world.mark_authored(guard).expect("authored");
        let bystander_id = world.mark_authored(bystander).expect("authored");
        let base_file = service.save_world(&world, goal).expect("the scene saves");

        world
            .get_mut::<Transform>(guard)
            .expect("placed")
            .translation = Vec3::new(0.0, 0.0, 5.0);
        let save = service
            .save_game(&world, AssetUUID::new(), &base_file, goal)
            .expect("the game saves");

        let mut editor = World::new();
        service
            .load_world(&base_file, &mut editor)
            .expect("the scene opens");
        let edited = editor.entity_with_id(bystander_id).expect("there");
        editor.get_mut::<Name>(edited).expect("named").0 = "Bystander, renamed".into();
        let edited_file = service.save_world(&editor, goal).expect("the scene saves");

        let mut loaded = World::new();
        service
            .load_game(&mut loaded, &save, &edited_file)
            .expect("the game loads");
        let back = loaded.entity_with_id(bystander_id).expect("there");
        assert_eq!(
            loaded.get::<Name>(back).map(Name::as_str),
            Some("Bystander, renamed"),
            "{goal:?}: the edit did not reach an entity the game never touched"
        );
    }
}

/// **A hand-damaged text scene under a save.** The text reader accepts a page
/// whose `Name` column lost a value; loading that scene is refused. Loading a
/// game over it must be refused too, not bring back a world missing the name.
#[test]
fn load_game_over_a_damaged_text_base_is_refused() {
    let service = SerializationService::new();
    let mut world = World::new();
    let guard = world.spawn((Transform::identity(), Name::new("Guard")));
    world.mark_authored(guard).expect("authored");
    let base_file = service
        .save_world(&world, SerializationGoal::LongTermStability)
        .expect("the scene saves");
    // The run spawns a pickup and leaves the guard alone.
    world.spawn((Transform::identity(), Name::new("Pickup")));
    let save = service
        .save_game(
            &world,
            AssetUUID::new(),
            &base_file,
            SerializationGoal::LongTermStability,
        )
        .expect("the game saves");

    // Damage the scene: the guard's `Name` column loses its value.
    let mut record = read_scene_file(&base_file).expect("reads");
    for page in &mut record.pages {
        if let Some(column) = page.components.iter().position(|name| name == "Name") {
            page.columns[column].clear();
        }
    }
    let damaged = write_scene_file(&record, &TextEncoding).expect("writes");
    assert!(
        service.load_world(&damaged, &mut World::new()).is_err(),
        "the damaged scene itself is refused"
    );

    let mut loaded = World::new();
    if let Ok(report) = service.load_game(&mut loaded, &save, &damaged) {
        panic!(
            "a game loaded over a damaged scene: {} entities, report {report:?}",
            loaded.iter_entities().count()
        );
    }
}

/// **Three runs on one scene, through files.** Each run loads the last save,
/// plays — spawning, destroying, moving — and saves again against the same
/// scene; the last load is the last world.
#[test]
fn three_save_generations_against_one_base_load_the_last_world() {
    let service = SerializationService::new();
    let mut world = World::new();
    let mut authored = Vec::new();
    for index in 0..4 {
        let entity = world.spawn((
            Transform::from_translation(Vec3::new(index as f32, 0.0, 0.0)),
            Name::new(format!("Authored {index}")),
        ));
        world.mark_authored(entity).expect("authored");
        authored.push(entity);
    }
    let base_file = service
        .save_world(&world, SerializationGoal::LongTermStability)
        .expect("the scene saves");

    let mut save = service
        .save_game(
            &world,
            AssetUUID::new(),
            &base_file,
            SerializationGoal::EditorInterchange,
        )
        .expect("the game saves");
    for run in 0..3 {
        let mut playing = World::new();
        service
            .load_game(&mut playing, &save, &base_file)
            .unwrap_or_else(|e| panic!("run {run}: {e}"));
        let living: Vec<_> = playing.iter_entities().collect();
        if let Some(first) = living.first() {
            if let Some(transform) = playing.get_mut::<Transform>(*first) {
                transform.translation.y += 1.0;
            }
        }
        if let Some(last) = living.last() {
            playing.despawn(*last);
        }
        playing.spawn((Transform::identity(), Name::new(format!("Spawned {run}"))));
        save = service
            .save_game(
                &playing,
                AssetUUID::new(),
                &base_file,
                SerializationGoal::EditorInterchange,
            )
            .unwrap_or_else(|e| panic!("run {run}: {e}"));

        let mut loaded = World::new();
        service
            .load_game(&mut loaded, &save, &base_file)
            .unwrap_or_else(|e| panic!("run {run}: {e}"));
        let names = |world: &World| {
            let mut names: Vec<(String, u32)> = world
                .iter_entities()
                .map(|entity| {
                    (
                        world
                            .get::<Name>(entity)
                            .map(|name| name.as_str().to_owned())
                            .unwrap_or_default(),
                        world
                            .get::<Transform>(entity)
                            .map(|t| t.translation.y.to_bits())
                            .unwrap_or_default(),
                    )
                })
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(&loaded), names(&playing), "run {run}");
    }
}
