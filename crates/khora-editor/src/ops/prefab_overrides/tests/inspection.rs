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

//! An inspected instance member against its prefab: the components it
//! overrides, adds and removes, and the prefab's values beside them.

use super::*;

/// The barrel pushed sideways: its transform overrides the translation's x,
/// and nothing else of the barrel differs. The prefab's transform is the one
/// the revert writes back.
#[test]
fn inspecting_a_member_reports_its_overridden_field() {
    let mut placed = placed();
    placed.translation_mut(placed.barrel).x = 5.0;

    let inspected = placed.inspect(placed.barrel);

    assert_eq!(inspected.root, placed.root);
    assert_eq!(inspected.prefab, placed.prefab);
    assert!(!inspected.is_root);
    assert_eq!(
        inspected.components,
        vec![ComponentOverride {
            type_name: "Transform".to_owned(),
            added: false,
            fields: vec![path(&["translation", "x"])],
        }]
    );
    assert!(inspected.removed.is_empty());
    assert_eq!(inspected.override_count(), 1);
    assert_eq!(
        inspected.fields_of("Transform"),
        &[path(&["translation", "x"])][..]
    );
    assert!(inspected.fields_of("Name").is_empty());
    let prefab_transform =
        prefab_value(&inspected, "Transform").expect("the prefab's transform is beside it");
    assert_eq!(prefab_transform["translation"]["x"], 0.0);
    assert_eq!(prefab_transform["translation"]["y"], 1.0);
    assert!(
        prefab_value(&inspected, "Name").is_some(),
        "every component the prefab gives the barrel"
    );
}

/// A tag the scene put on the sight is a component the instance added: no
/// field paths, no prefab value.
#[test]
fn inspecting_a_member_reports_a_component_it_added() {
    let mut placed = placed();
    placed
        .world
        .inner_world_mut()
        .add_component(placed.sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");

    let inspected = placed.inspect(placed.sight);

    assert_eq!(
        inspected.components,
        vec![ComponentOverride {
            type_name: "Tag".to_owned(),
            added: true,
            fields: Vec::new(),
        }]
    );
    assert!(inspected.removed.is_empty());
    assert_eq!(inspected.override_count(), 1);
    assert!(inspected.fields_of("Tag").is_empty());
    assert!(
        prefab_value(&inspected, "Tag").is_none(),
        "the prefab gives the sight no tag"
    );
}

/// The light taken off the root is a component the instance removed; the
/// prefab's light is kept beside it, to bring it back. The root's link to
/// its prefab is the instance's own, not an added component.
#[test]
fn inspecting_the_root_reports_a_component_it_removed() {
    let mut placed = placed();
    placed
        .world
        .inner_world_mut()
        .remove_component::<Light>(placed.root)
        .expect("the light comes off");

    let inspected = placed.inspect(placed.root);

    assert!(inspected.is_root);
    assert_eq!(inspected.removed, vec!["Light".to_owned()]);
    assert_eq!(
        inspected.components,
        Vec::new(),
        "neither the link nor anything else is an override"
    );
    assert_eq!(inspected.override_count(), 1);
    let light = prefab_value(&inspected, "Light").expect("the prefab's light is beside it");
    assert!(
        light["light_type"].get("Point").is_some(),
        "the prefab's point light: {light}"
    );
}

/// A member left alone overrides nothing — though it carries the link, or a
/// component the engine derives that the prefab never holds.
#[test]
fn an_untouched_member_overrides_nothing() {
    let mut placed = placed();
    placed
        .world
        .inner_world_mut()
        .add_component(placed.barrel, GlobalTransform::identity())
        .expect("a derived transform attaches");

    for (what, member) in [("root", placed.root), ("barrel", placed.barrel)] {
        let inspected = placed.inspect(member);
        assert!(
            inspected.components.is_empty(),
            "the {what}: {:?}",
            inspected.components
        );
        assert!(inspected.removed.is_empty(), "the {what}");
        assert_eq!(inspected.override_count(), 0, "the {what}");
    }
}

/// The count sums every override of the entity: each overridden field, each
/// added component, each removed one.
#[test]
fn the_override_count_sums_fields_and_added_and_removed_components() {
    let mut placed = placed();
    let inner = placed.world.inner_world_mut();
    let transform = inner.get_mut::<Transform>(placed.root).expect("placed");
    transform.scale.x = 2.0;
    transform.scale.z = 3.0;
    inner
        .add_component(placed.root, Tag::from_iter(["armed"]))
        .expect("a tag attaches");
    inner
        .remove_component::<Light>(placed.root)
        .expect("the light comes off");

    let inspected = placed.inspect(placed.root);

    assert_eq!(
        sorted(inspected.fields_of("Transform").to_vec()),
        vec![path(&["scale", "x"]), path(&["scale", "z"])]
    );
    assert_eq!(
        inspected.override_count(),
        4,
        "two fields, one added, one removed"
    );
}

/// The root's placement — where the instance stands in its scene, its
/// translation, its rotation and its parent — is the instance's own, never
/// an override of the prefab; its scale is one, as any value of a member.
#[test]
fn the_roots_placement_is_no_override() {
    let mut placed = placed();
    let base = placed
        .world
        .spawn((Transform::identity(), Name::new("Base")));
    placed
        .world
        .inner_world_mut()
        .mark_authored(base)
        .expect("authored");
    placed.world.set_parent(placed.root, Some(base));
    {
        let transform = placed
            .world
            .get_component_mut::<Transform>(placed.root)
            .expect("placed");
        transform.translation = Vec3::new(10.0, 0.0, -4.0);
        transform.rotation = khora_sdk::prelude::math::Quaternion::from_axis_angle(Vec3::Y, 0.5);
    }

    let inspected = placed.inspect(placed.root);
    assert!(
        inspected.components.is_empty(),
        "moved, turned and parented: {:?}",
        inspected.components
    );
    assert_eq!(inspected.override_count(), 0);

    placed
        .world
        .get_component_mut::<Transform>(placed.root)
        .expect("placed")
        .scale
        .y = 2.0;
    let inspected = placed.inspect(placed.root);
    assert_eq!(
        inspected.fields_of("Transform"),
        &[path(&["scale", "y"])][..],
        "the scale is an override"
    );
    assert_eq!(inspected.override_count(), 1);
}

/// The count and the field lookup read what the inspected view holds.
#[test]
fn the_override_count_and_field_lookup_read_the_view() {
    let view = InspectedPrefab {
        root: EntityId {
            index: 1,
            generation: 0,
        },
        prefab: AssetUUID::new(),
        prefab_path: Some("prefabs/turret.kprefab".to_owned()),
        is_root: false,
        components: vec![
            ComponentOverride {
                type_name: "Transform".to_owned(),
                added: false,
                fields: vec![path(&["translation", "x"]), path(&["scale", "y"])],
            },
            ComponentOverride {
                type_name: "Tag".to_owned(),
                added: true,
                fields: Vec::new(),
            },
        ],
        removed: vec!["Light".to_owned(), "AudioSource".to_owned()],
        prefab_json: Vec::new(),
    };

    assert_eq!(view.override_count(), 5);
    assert_eq!(
        view.fields_of("Transform"),
        &[path(&["translation", "x"]), path(&["scale", "y"])][..]
    );
    assert!(view.fields_of("Tag").is_empty());
    assert!(
        view.fields_of("Camera").is_empty(),
        "a component not overridden"
    );
}

/// An entity that is no part of an instance has no prefab to be seen
/// against: a plain one, and one the author hung under a member.
#[test]
fn an_entity_outside_any_instance_has_no_prefab_view() {
    let mut placed = placed();
    let lone = placed
        .world
        .spawn((Transform::identity(), Name::new("Lone")));
    let muzzle = placed
        .world
        .spawn((Transform::identity(), Name::new("Muzzle")));
    placed.world.set_parent(muzzle, Some(placed.sight));
    for entity in [lone, muzzle] {
        placed
            .world
            .inner_world_mut()
            .mark_authored(entity)
            .expect("authored");
    }

    assert!(inspect_prefab(&placed.world, lone, &placed.library).is_none());
    assert!(
        inspect_prefab(&placed.world, muzzle, &placed.library).is_none(),
        "the author's entity under the sight"
    );
}

/// A member's parent is the instance root in the scene and the prefab root
/// in the prefab's expansion — the same entity of the prefab, whatever index
/// each world gives it: no override.
#[test]
fn a_members_parent_is_no_override_wherever_the_instance_sits_in_the_scene() {
    let placed = placed_after_others();

    for (what, member) in [("barrel", placed.barrel), ("sight", placed.sight)] {
        let inspected = placed.inspect(member);
        assert!(
            inspected.components.is_empty(),
            "the untouched {what}: {:?}",
            inspected.components
        );
        assert_eq!(inspected.override_count(), 0, "the untouched {what}");
    }
}

/// A prefab whose script aims outside it — at an entity of the scene it was
/// made in — holds a reference to nothing, and so does every instance of
/// it: an untouched instance overrides nothing there.
#[test]
fn a_reference_to_nothing_is_no_override() {
    let placed = scripted(|made, _, _| made.spawn((Transform::identity(), Name::new("Lone"))));

    let inspected = inspect_prefab(&placed.world, placed.root, &placed.library)
        .expect("the root belongs to its instance");

    assert!(
        inspected.components.is_empty(),
        "the untouched root: {:?}",
        inspected.components
    );
    assert_eq!(inspected.override_count(), 0);
}
