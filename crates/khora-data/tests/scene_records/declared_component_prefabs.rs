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

//! A prefab holding a declared component: its instances carry it, in a
//! world that never declared it, and an instance's override of one of its
//! fields survives the scene being saved as a link and opened again.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Name, SemanticDomain, Transform};
use khora_data::scene::{capture_subtree, instantiate_prefab};

use super::declared_components::{attach, declare_both, domain_of, field_of, fields_of, set_field};
use super::prefab_sample::{entity, id, open, save, Library};
use super::*;

/// A beacon prefab: a root holding a `Health` that targets its lamp child,
/// and the lamp — by their ids in the prefab.
struct Beacon {
    record: SceneRecord,
    root: PersistentId,
    lamp: PersistentId,
}

fn beacon() -> Beacon {
    let mut world = World::new();
    declare_both(&mut world);
    let root = world.spawn((Transform::identity(), Name::new("Beacon")));
    let lamp = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
        Name::new("Lamp"),
    ));
    world.set_parent(lamp, Some(root));
    attach(
        &mut world,
        root,
        "Health",
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Int(60)),
            ("label".into(), ScriptValue::Str("beacon".into())),
            ("target".into(), ScriptValue::Entity(lamp)),
        ]),
    );
    for entity in [root, lamp] {
        world.mark_authored(entity).expect("authored");
    }
    let record = capture_subtree(&world, root).expect("the beacon captures");
    Beacon {
        record,
        root: id(&world, root),
        lamp: id(&world, lamp),
    }
}

/// An instance of a prefab holding a declared component brings it, into a
/// world that never declared it: in the prefab's domain, its values as the
/// prefab holds them, its reference pointing inside the instance.
#[test]
fn a_prefab_instance_brings_its_declared_components() {
    let beacon = beacon();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, beacon.record.clone());
    let mut world = World::new();

    let root = instantiate_prefab(&mut world, prefab, &library).expect("the beacon instantiates");
    let instance = id(&world, root);
    let lamp = entity(&world, PersistentId::within(instance, beacon.lamp));

    assert_eq!(
        field_of(&world, root, "Health", "current"),
        Some(ScriptValue::Int(60))
    );
    assert_eq!(
        field_of(&world, root, "Health", "label"),
        Some(ScriptValue::Str("beacon".into()))
    );
    assert_eq!(
        field_of(&world, root, "Health", "max"),
        Some(ScriptValue::Int(100)),
        "a field the prefab left at its default"
    );
    assert_eq!(
        field_of(&world, root, "Health", "target"),
        Some(ScriptValue::Entity(lamp)),
        "a reference inside the prefab points inside the instance"
    );
    assert_eq!(domain_of(&world, "Health"), Some(SemanticDomain::Physics));
    assert_eq!(fields_of(&world, lamp, "Health"), None);
}

/// An instance whose `Health` the author changed is saved as a link plus
/// that change, and opens again — in a fresh world, through every encoding
/// — with the change on top of the prefab's values.
#[test]
fn an_instance_override_of_a_declared_field_survives_a_save() {
    let beacon = beacon();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, beacon.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the beacon instantiates");
    let instance = id(&world, root);
    set_field(&mut world, root, "Health", "current", ScriptValue::Int(5));

    let scene = save(&world, &library);
    assert_eq!(scene.instances.len(), 1, "the instance is kept as a link");
    for (name, encoding) in every_encoding() {
        let back = through(&scene, name, encoding);
        let opened = open(&back, &library);
        let root = entity(&opened, instance);
        let lamp = entity(&opened, PersistentId::within(instance, beacon.lamp));
        assert_eq!(
            field_of(&opened, root, "Health", "current"),
            Some(ScriptValue::Int(5)),
            "{name}: the instance's override"
        );
        assert_eq!(
            field_of(&opened, root, "Health", "label"),
            Some(ScriptValue::Str("beacon".into())),
            "{name}: the prefab's value beneath it"
        );
        assert_eq!(
            field_of(&opened, root, "Health", "target"),
            Some(ScriptValue::Entity(lamp)),
            "{name}"
        );
    }
}

/// A prefab edited after an instance was saved reaches the instance's
/// declared fields the instance left alone.
#[test]
fn a_prefab_edit_reaches_an_instance_declared_field() {
    let beacon = beacon();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, beacon.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the beacon instantiates");
    let instance = id(&world, root);
    let scene = save(&world, &library);

    let (edited, ()) = super::prefab_sample::edited(&beacon.record, |prefab_world| {
        let root = entity(prefab_world, beacon.root);
        set_field(prefab_world, root, "Health", "max", ScriptValue::Int(250));
    });
    let opened = open(&scene, &Library::default().with(prefab, edited));
    let root = entity(&opened, instance);
    assert_eq!(
        field_of(&opened, root, "Health", "max"),
        Some(ScriptValue::Int(250))
    );
    assert_eq!(
        field_of(&opened, root, "Health", "current"),
        Some(ScriptValue::Int(60))
    );
}

/// A beacon prefab whose root holds a `Health` declared with `fields`, in
/// `Physics`, its `current` set to `current`.
fn beacon_declared(
    fields: Vec<khora_data::ecs::PackedField>,
    current: ScriptValue,
) -> (SceneRecord, PersistentId) {
    let mut world = World::new();
    super::declared_components::declare(&mut world, "Health", SemanticDomain::Physics, fields);
    let root = world.spawn((Transform::identity(), Name::new("Beacon")));
    attach(
        &mut world,
        root,
        "Health",
        ScriptValue::Struct(vec![("current".into(), current)]),
    );
    world.mark_authored(root).expect("authored");
    let record = capture_subtree(&world, root).expect("the beacon captures");
    let root = id(&world, root);
    (record, root)
}

/// `Health` with `current` of `kind`, the other fields as the scripts
/// declare them.
fn health_with_current(
    kind: khora_data::ecs::FieldKind,
    default: ScriptValue,
) -> Vec<khora_data::ecs::PackedField> {
    let mut fields = super::declared_components::health_fields();
    fields[0] = super::declared_components::field("current", kind, default);
    fields
}

/// A scene and a prefab it links to may have been saved under different
/// declarations of one component — the prefab re-saved after a script
/// changed `Health.current` from a `float` to an `int`, the scene not. Each
/// cell is read as the file that wrote it declared it: the scene opens, its
/// own entity keeps its `Health`, and the instance holds the prefab's.
#[test]
fn a_scene_opens_when_its_prefab_declares_a_component_differently() {
    use khora_data::ecs::FieldKind;
    let prefab = AssetUUID::new();
    let (float_beacon, _) = beacon_declared(
        health_with_current(FieldKind::Float, ScriptValue::Float(100.0)),
        ScriptValue::Float(60.0),
    );
    let float_library = Library::default().with(prefab, float_beacon);

    // The scene, saved while `current` was a float.
    let mut world = World::new();
    super::declared_components::declare(
        &mut world,
        "Health",
        SemanticDomain::Physics,
        health_with_current(FieldKind::Float, ScriptValue::Float(100.0)),
    );
    let root =
        instantiate_prefab(&mut world, prefab, &float_library).expect("the beacon instantiates");
    let instance = id(&world, root);
    let post = world.spawn((Transform::identity(), Name::new("Post")));
    attach(
        &mut world,
        post,
        "Health",
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Float(12.5)),
            ("label".into(), ScriptValue::Str("post".into())),
        ]),
    );
    world.mark_authored(post).expect("authored");
    let post = id(&world, post);
    let scene = save(&world, &float_library);

    // The prefab, re-saved once `current` became an int.
    let (int_beacon, _) = beacon_declared(
        health_with_current(FieldKind::Int, ScriptValue::Int(100)),
        ScriptValue::Int(60),
    );
    let int_library = Library::default().with(prefab, int_beacon);

    let expanded = khora_data::scene::expand(&scene, &int_library)
        .unwrap_or_else(|e| panic!("the scene does not expand: {e}"));
    let mut opened = World::new();
    apply(&mut opened, &expanded, Identity::Keep).unwrap_or_else(|e| {
        panic!("a scene whose prefab declares `Health` differently does not load: {e}")
    });
    assert_eq!(
        field_of(&opened, entity(&opened, post), "Health", "label"),
        Some(ScriptValue::Str("post".into())),
        "the scene's own entity keeps its `Health`"
    );
    assert!(
        fields_of(&opened, entity(&opened, instance), "Health").is_some(),
        "the instance holds the prefab's `Health`"
    );
}

/// A subtree duplicated twice: each copy's declared components point inside
/// that copy — a plain entity field, and entities held in a list in a field
/// of any value — while a reference to an entity outside the subtree points
/// at no entity, in either copy.
#[test]
fn a_duplicated_subtree_remaps_the_entities_its_declared_components_hold() {
    use khora_data::scene::{instantiate_subtree, serialize_subtree, NoPrefabs};
    let mut world = World::new();
    declare_both(&mut world);
    let outsider = world.spawn(Transform::identity());
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    world.set_parent(lamp, Some(root));
    attach(
        &mut world,
        root,
        "Health",
        ScriptValue::Struct(vec![
            ("target".into(), ScriptValue::Entity(lamp)),
            ("ally".into(), ScriptValue::Entity(outsider)),
        ]),
    );
    attach(
        &mut world,
        root,
        "Mood",
        ScriptValue::Struct(vec![(
            "tags".into(),
            ScriptValue::Array(vec![ScriptValue::Entity(lamp), ScriptValue::Entity(root)]),
        )]),
    );
    for entity in [outsider, root, lamp] {
        world.mark_authored(entity).expect("authored");
    }
    let bytes = serialize_subtree(&world, root, &NoPrefabs).expect("the subtree serializes");

    for copy in 0..2 {
        let made = instantiate_subtree(&mut world, &bytes, &NoPrefabs)
            .unwrap_or_else(|e| panic!("copy {copy}: the subtree does not instantiate: {e}"));
        assert_ne!(made, root, "copy {copy}");
        let made_lamp = world
            .get::<khora_data::ecs::Children>(made)
            .and_then(|children| children.0.first().copied())
            .unwrap_or_else(|| panic!("copy {copy}: the copy has its lamp"));
        assert_ne!(made_lamp, lamp, "copy {copy}");
        assert_eq!(
            field_of(&world, made, "Health", "target"),
            Some(ScriptValue::Entity(made_lamp)),
            "copy {copy}: an entity field points inside the copy"
        );
        assert_eq!(
            field_of(&world, made, "Mood", "tags"),
            Some(ScriptValue::Array(vec![
                ScriptValue::Entity(made_lamp),
                ScriptValue::Entity(made),
            ])),
            "copy {copy}: entities in a list point inside the copy"
        );
        let ally = field_of(&world, made, "Health", "ally");
        assert!(
            !matches!(ally, Some(ScriptValue::Entity(e)) if world.contains(e)),
            "copy {copy}: a reference outside the subtree names no live entity: {ally:?}"
        );
    }
}

/// A prefab nesting another, its nested instance given a declared component
/// the inner prefab lacks: a scene holding the outer prefab opens, in a
/// world that declares nothing, through every encoding of every file, with
/// the inner prefab's declared component and the nested override both on
/// the nested instance's root.
#[test]
fn a_nested_prefab_brings_its_declared_components_and_overrides() {
    use khora_data::scene::collapse;
    let beacon = beacon();
    let beacon_id = AssetUUID::new();
    let inner = Library::default().with(beacon_id, beacon.record.clone());

    // The tower: a root, the beacon nested under it, given a `Mood`.
    let mut world = World::new();
    declare_both(&mut world);
    let tower_root = world.spawn((Transform::identity(), Name::new("Tower")));
    world.mark_authored(tower_root).expect("authored");
    let nested = instantiate_prefab(&mut world, beacon_id, &inner).expect("the beacon nests");
    world.set_parent(nested, Some(tower_root));
    attach(
        &mut world,
        nested,
        "Mood",
        ScriptValue::Struct(vec![("n".into(), ScriptValue::Int(5))]),
    );
    let nested_id = id(&world, nested);
    let tower = collapse(
        &capture_subtree(&world, tower_root).expect("captures"),
        &inner,
    )
    .unwrap_or_else(|e| panic!("the tower does not collapse: {e}"));
    assert_eq!(tower.instances.len(), 1, "the beacon is nested as a link");
    let tower_id = AssetUUID::new();

    for (name, encoding) in every_encoding() {
        let library = Library::default()
            .with(beacon_id, through(&beacon.record, name, encoding))
            .with(tower_id, through(&tower, name, encoding));
        let mut scene_world = World::new();
        let made = instantiate_prefab(&mut scene_world, tower_id, &library)
            .unwrap_or_else(|e| panic!("{name}: the tower does not instantiate: {e}"));
        let scene = through(&save(&scene_world, &library), name, encoding);
        let opened = open(&scene, &library);
        let instance = id(&scene_world, made);
        let root = entity(&opened, PersistentId::within(instance, nested_id));
        assert_eq!(
            field_of(&opened, root, "Health", "current"),
            Some(ScriptValue::Int(60)),
            "{name}: the inner prefab's declared component"
        );
        assert_eq!(
            field_of(&opened, root, "Mood", "n"),
            Some(ScriptValue::Int(5)),
            "{name}: the nested instance's declared override"
        );
    }
}
