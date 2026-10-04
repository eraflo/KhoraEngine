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

//! Reverting an override is an ordinary edit: a field set back through the
//! component's JSON, a removed component inserted again with its value, an
//! added one removed — undoable like any edit.

use crate::commands::history::EditorCommand;
use crate::commands::CommandHistory;

use super::*;

/// A light's JSON view, switched off — a value no default would give.
fn switched_off_light(world: &mut GameWorld) -> Value {
    let probe = world.spawn((Transform::identity(), Light::point()));
    let mut value = component_json(world, probe, "Light").expect("the light's JSON view");
    world.despawn(probe);
    value["enabled"] = Value::Bool(false);
    value
}

fn lit(world: &GameWorld, entity: EntityId) -> Option<bool> {
    world
        .get_component::<Light>(entity)
        .map(|light| light.enabled)
}

/// Inserting a component the entity lacks adds it with the value the edit
/// carries, not a default.
#[test]
fn inserting_a_component_restores_it_with_its_value() {
    let mut world = GameWorld::new();
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    let value = switched_off_light(&mut world);

    apply(
        &mut world,
        vec![PropertyEdit::InsertComponentJson {
            entity: lamp,
            type_name: "Light".to_owned(),
            value: value.clone(),
        }],
    );

    assert_eq!(lit(&world, lamp), Some(false), "the light, switched off");
    assert_eq!(component_json(&world, lamp, "Light"), Some(value));
}

/// Inserting a component the entity already has sets it to the edit's
/// value.
#[test]
fn inserting_a_component_the_entity_has_sets_its_value() {
    let mut world = GameWorld::new();
    let lamp = world.spawn((Transform::identity(), Light::point()));
    let value = switched_off_light(&mut world);

    apply(
        &mut world,
        vec![PropertyEdit::InsertComponentJson {
            entity: lamp,
            type_name: "Light".to_owned(),
            value,
        }],
    );

    assert_eq!(lit(&world, lamp), Some(false));
}

/// Taking a component off is undone by inserting it again, with the value
/// it had — and redone by taking it off again.
#[test]
fn removing_a_component_is_undone_by_inserting_it_back() {
    let mut world = GameWorld::new();
    let value = switched_off_light(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    apply(
        &mut world,
        vec![PropertyEdit::InsertComponentJson {
            entity: lamp,
            type_name: "Light".to_owned(),
            value: value.clone(),
        }],
    );
    let mut history = CommandHistory::default();
    let remove = PropertyEdit::RemoveComponent {
        entity: lamp,
        type_name: "Light".to_owned(),
    };
    history.push(EditorCommand {
        description: "Remove Light".to_owned(),
        forward: remove.clone(),
        reverse: PropertyEdit::InsertComponentJson {
            entity: lamp,
            type_name: "Light".to_owned(),
            value,
        },
    });
    apply(&mut world, vec![remove]);
    assert_eq!(lit(&world, lamp), None);

    apply(&mut world, vec![history.undo().expect("a command to undo")]);
    assert_eq!(lit(&world, lamp), Some(false), "back, switched off");

    apply(&mut world, vec![history.redo().expect("a command to redo")]);
    assert_eq!(lit(&world, lamp), None, "off again");
}

/// Bringing a removed component back is itself undone by removing it.
#[test]
fn inserting_a_component_is_undone_by_removing_it() {
    let mut world = GameWorld::new();
    let value = switched_off_light(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    let insert = PropertyEdit::InsertComponentJson {
        entity: lamp,
        type_name: "Light".to_owned(),
        value,
    };
    let mut history = CommandHistory::default();
    history.push(EditorCommand {
        description: "Revert Light".to_owned(),
        forward: insert.clone(),
        reverse: PropertyEdit::RemoveComponent {
            entity: lamp,
            type_name: "Light".to_owned(),
        },
    });
    apply(&mut world, vec![insert]);
    assert_eq!(lit(&world, lamp), Some(false));

    apply(&mut world, vec![history.undo().expect("a command to undo")]);
    assert_eq!(lit(&world, lamp), None);

    apply(&mut world, vec![history.redo().expect("a command to redo")]);
    assert_eq!(lit(&world, lamp), Some(false));
}

/// Reverting one field of the barrel — the prefab's value set into the live
/// JSON — clears that override and keeps the other.
#[test]
fn reverting_a_field_clears_its_override_and_keeps_the_others() {
    let mut placed = placed();
    *placed.translation_mut(placed.barrel) = Vec3::new(5.0, 7.0, 0.0);
    let inspected = placed.inspect(placed.barrel);
    let live = component_json(&placed.world, placed.barrel, "Transform").expect("placed");
    let prefab = prefab_value(&inspected, "Transform")
        .expect("the prefab's transform")
        .clone();

    apply(
        &mut placed.world,
        vec![PropertyEdit::SetComponentJson {
            entity: placed.barrel,
            type_name: "Transform".to_owned(),
            value: reverted(&live, &prefab, &path(&["translation", "x"])),
        }],
    );

    assert_eq!(
        *placed.translation_mut(placed.barrel),
        Vec3::new(0.0, 7.0, 0.0)
    );
    assert_eq!(
        placed.inspect(placed.barrel).fields_of("Transform"),
        &[path(&["translation", "y"])][..]
    );
}

/// Reverting a removed light inserts the prefab's light again: the root no
/// longer overrides anything.
#[test]
fn reverting_a_removed_component_brings_back_the_prefabs() {
    let mut placed = placed();
    placed
        .world
        .inner_world_mut()
        .remove_component::<Light>(placed.root)
        .expect("the light comes off");
    let inspected = placed.inspect(placed.root);
    let light = prefab_value(&inspected, "Light")
        .expect("the prefab's light")
        .clone();

    apply(
        &mut placed.world,
        vec![PropertyEdit::InsertComponentJson {
            entity: placed.root,
            type_name: "Light".to_owned(),
            value: light,
        }],
    );

    let inspected = placed.inspect(placed.root);
    assert!(inspected.removed.is_empty(), "{:?}", inspected.removed);
    assert_eq!(inspected.override_count(), 0);
}

/// Reverting an added tag removes it: the sight no longer overrides
/// anything.
#[test]
fn reverting_an_added_component_removes_it() {
    let mut placed = placed();
    placed
        .world
        .inner_world_mut()
        .add_component(placed.sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");
    assert_eq!(placed.inspect(placed.sight).override_count(), 1);

    apply(
        &mut placed.world,
        vec![PropertyEdit::RemoveComponent {
            entity: placed.sight,
            type_name: "Tag".to_owned(),
        }],
    );

    assert_eq!(placed.inspect(placed.sight).override_count(), 0);
}

/// The edits that take an instance back to its prefab, applied.
fn revert_all(placed: &mut Placed) {
    let instance = InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    };
    let scratch = prefab_world(placed.world.inner_world(), &instance, &placed.library)
        .expect("the prefab expands");
    let edits = revert_instance(placed.world.inner_world(), &instance, &scratch);
    apply(&mut placed.world, edits);
}

/// Reverting the whole instance takes every member back to the prefab — a
/// pushed barrel, a tag on the sight, the light taken off the root — and
/// leaves the root where the scene put it.
#[test]
fn reverting_the_instance_takes_every_member_back_but_the_roots_placement() {
    let mut placed = placed();
    let placement = Vec3::new(10.0, 0.0, -4.0);
    *placed.translation_mut(placed.root) = placement;
    placed
        .world
        .get_component_mut::<Transform>(placed.root)
        .expect("placed")
        .scale
        .y = 2.0;
    placed.translation_mut(placed.barrel).x = 5.0;
    let inner = placed.world.inner_world_mut();
    inner
        .add_component(placed.sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");
    inner
        .remove_component::<Light>(placed.root)
        .expect("the light comes off");

    revert_all(&mut placed);

    for (what, member) in [
        ("root", placed.root),
        ("barrel", placed.barrel),
        ("sight", placed.sight),
    ] {
        assert_eq!(placed.inspect(member).override_count(), 0, "the {what}");
    }
    assert_eq!(*placed.translation_mut(placed.root), placement);
    assert!(placed.world.get_component::<Light>(placed.root).is_some());
}

/// Reverting an instance placed after other entities keeps each member under
/// its own parent: an instance that overrides nothing is left as it is.
#[test]
fn reverting_an_untouched_instance_keeps_its_hierarchy() {
    let mut placed = placed_after_others();
    let instance = InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    };
    let scratch = prefab_world(placed.world.inner_world(), &instance, &placed.library)
        .expect("the prefab expands");

    let edits = revert_instance(placed.world.inner_world(), &instance, &scratch);
    assert!(edits.is_empty(), "nothing to revert: {edits:?}");

    revert_all(&mut placed);
    for member in [placed.barrel, placed.sight] {
        assert_eq!(
            placed
                .world
                .get_component::<Parent>(member)
                .map(|parent| parent.0),
            Some(placed.root),
            "the member stays under the root"
        );
    }
}

/// The instance deleted the barrel its script aims at — structure, which a
/// revert leaves as it is — and changed the script's range. Reverting the
/// instance still takes the range back to the prefab's.
#[test]
fn reverting_the_instance_reverts_a_script_aiming_at_a_deleted_member() {
    use khora_sdk::khora_core::script::ScriptValue;
    use khora_sdk::khora_data::ecs::Script;

    let mut placed = scripted(|_, _, barrel| barrel);
    placed.world.despawn(placed.barrel);
    {
        let script = placed
            .world
            .get_component_mut::<Script>(placed.root)
            .expect("scripted");
        *script = script.clone().with_field("range", ScriptValue::Float(9.0));
    }
    let instance = InstanceOf {
        root: placed.root,
        prefab: placed.prefab,
    };
    let scratch = prefab_world(placed.world.inner_world(), &instance, &placed.library)
        .expect("the prefab expands");

    let edits = revert_instance(placed.world.inner_world(), &instance, &scratch);
    apply(&mut placed.world, edits);

    assert_eq!(script_range(&placed.world, placed.root), Some(5.0));
}

/// A barrel the author took out to the scene's root has lost the parent the
/// prefab gives it. Reverting the instance hangs it back under the root —
/// both halves of the edge: the barrel's parent and the root's children.
#[test]
fn reverting_the_instance_rehangs_a_detached_member_on_both_halves_of_the_edge() {
    let mut placed = placed();
    placed.world.set_parent(placed.barrel, None);

    revert_all(&mut placed);

    assert_eq!(
        placed
            .world
            .get_component::<Parent>(placed.barrel)
            .map(|parent| parent.0),
        Some(placed.root),
        "the barrel's parent"
    );
    assert!(
        placed
            .world
            .get_component::<Children>(placed.root)
            .is_some_and(|children| children.0.contains(&placed.barrel)),
        "the root's children"
    );
}
