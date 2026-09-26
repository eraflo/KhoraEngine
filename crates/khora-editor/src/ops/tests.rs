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

use super::*;

/// Captures the live JSON of a component on `entity` via the inventory
/// registration — mirrors exactly what the inspector renders, so edits
/// built on top of it patch the same shape `from_json` consumes.
fn component_json(
    world: &GameWorld,
    entity: EntityId,
    type_name: &str,
) -> Option<serde_json::Value> {
    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        if reg.type_name == type_name {
            return (reg.to_json)(world.inner_world(), entity);
        }
    }
    None
}

/// Inspector edit → commit: a queued `SetName` plus a `SetComponentJson`
/// (patching a real component's field through its JSON shape) must land in
/// the live `World` once `apply_edits` runs. This is the editor's single
/// mutation path; if `drain_edits` / registry dispatch / `from_json` break,
/// inspector edits silently no-op.
#[test]
fn apply_edits_commits_name_and_component_json() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let entity = world.spawn((
        Transform::from_translation(khora_sdk::prelude::math::Vec3::new(1.0, 2.0, 3.0)),
        GlobalTransform::identity(),
        Name::new("Before"),
    ));

    // Patch the Transform translation through its JSON shape, exactly as
    // the inspector does: read the live value, mutate one field, ship back.
    let mut transform_json =
        component_json(&world, entity, "Transform").expect("Transform JSON view");
    transform_json["translation"]["x"] = serde_json::json!(9.0);

    state.push_edit(PropertyEdit::SetName(entity, "After".to_owned()));
    state.push_edit(PropertyEdit::SetComponentJson {
        entity,
        type_name: "Transform".to_owned(),
        value: transform_json,
    });

    apply_edits(&mut world, &mut state);

    assert_eq!(
        world.get_component::<Name>(entity).map(|n| n.as_str()),
        Some("After"),
        "SetName must rename the entity"
    );
    let t = world
        .get_component::<Transform>(entity)
        .expect("entity keeps its Transform");
    assert_eq!(
        t.translation.x, 9.0,
        "SetComponentJson must patch the field"
    );
    assert_eq!(t.translation.y, 2.0, "untouched fields must survive");

    // Edits are drained — a second apply is a no-op.
    assert!(state.pending_edits.is_empty());
}

/// Undo/redo driven through the real apply path: push a forward edit to the
/// history and apply it, then `undo()` → apply reverse → back to baseline,
/// then `redo()` → apply forward → changed again. The history state machine
/// is unit-tested elsewhere; this locks in its integration with
/// `apply_edits`.
#[test]
fn undo_redo_roundtrips_through_apply_edits() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();
    let mut history = khora_sdk::editor_ui::CommandHistory::default();

    let entity = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Baseline"),
    ));

    let forward = PropertyEdit::SetName(entity, "Renamed".to_owned());
    let reverse = PropertyEdit::SetName(entity, "Baseline".to_owned());
    history.push(khora_sdk::editor_ui::EditorCommand {
        description: "Rename".to_owned(),
        forward: forward.clone(),
        reverse,
    });

    // Apply forward.
    state.push_edit(forward);
    apply_edits(&mut world, &mut state);
    assert_eq!(
        world.get_component::<Name>(entity).map(|n| n.as_str()),
        Some("Renamed")
    );

    // Undo → apply the reverse edit the history hands back.
    let reverse_edit = history.undo().expect("a command to undo");
    state.push_edit(reverse_edit);
    apply_edits(&mut world, &mut state);
    assert_eq!(
        world.get_component::<Name>(entity).map(|n| n.as_str()),
        Some("Baseline"),
        "undo must restore the baseline name"
    );

    // Redo → re-apply the forward edit.
    let forward_again = history.redo().expect("a command to redo");
    state.push_edit(forward_again);
    apply_edits(&mut world, &mut state);
    assert_eq!(
        world.get_component::<Name>(entity).map(|n| n.as_str()),
        Some("Renamed"),
        "redo must re-apply the change"
    );
}

/// `process_reparents` must wire both sides of the hierarchy: the child
/// gains a `Parent`, the parent gains the child in its `Children` list.
#[test]
fn process_reparents_links_parent_and_child() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let parent = world.spawn((Transform::identity(), GlobalTransform::identity()));
    let child = world.spawn((Transform::identity(), GlobalTransform::identity()));

    state.pending_reparent = Some((child, Some(parent)));
    process_reparents(&mut world, &mut state);

    assert_eq!(
        world.get_component::<Parent>(child).map(|p| p.0),
        Some(parent),
        "child must reference its new parent"
    );
    let children = world
        .get_component::<Children>(parent)
        .expect("parent gains a Children list");
    assert!(
        children.0.contains(&child),
        "parent's Children must include the reparented child"
    );

    // Detach back to root: Parent drops, parent's Children empties.
    state.pending_reparent = Some((child, None));
    process_reparents(&mut world, &mut state);
    assert!(
        world.get_component::<Parent>(child).is_none(),
        "detached child must lose its Parent"
    );
    let children = world.get_component::<Children>(parent).unwrap();
    assert!(
        !children.0.contains(&child),
        "former parent must drop the detached child"
    );
}

/// Spawns `A → B → C` and returns the three ids, in depth order.
fn spawn_three_level_chain(
    world: &mut GameWorld,
    state: &mut EditorState,
) -> (EntityId, EntityId, EntityId) {
    let a = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("A"),
    ));
    let b = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("B"),
    ));
    let c = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("C"),
    ));
    state.pending_reparent = Some((b, Some(a)));
    process_reparents(world, state);
    state.pending_reparent = Some((c, Some(b)));
    process_reparents(world, state);
    (a, b, c)
}

/// A three-level hierarchy must come out as one root with the full chain
/// nested under it.
///
/// The old bottom-up fold drained a `HashMap`, so when `(B,A)` happened to
/// be processed before `(C,B)`, `B` had already been moved into `A` and the
/// lookup for `C`'s parent missed — re-rooting `C` at the top level.
#[test]
fn extract_scene_tree_nests_three_levels() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();
    let (a, b, c) = spawn_three_level_chain(&mut world, &mut state);

    extract_scene_tree(&world, &mut state);

    assert_eq!(state.scene_roots.len(), 1, "only A is a root");
    let root = &state.scene_roots[0];
    assert_eq!(root.entity, a);
    assert_eq!(root.children.len(), 1, "A owns B");
    assert_eq!(root.children[0].entity, b);
    assert_eq!(root.children[0].children.len(), 1, "B owns C");
    assert_eq!(root.children[0].children[0].entity, c);
}

/// The extracted tree must be identical on every extraction. `HashMap`
/// re-seeds its hasher per instance, so an order-dependent build produced a
/// different shape each frame — rows visibly jittered and a click could
/// land on the wrong entity.
#[test]
fn extract_scene_tree_is_stable_across_extractions() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();
    spawn_three_level_chain(&mut world, &mut state);

    // Several root-level siblings exercise sibling ordering too.
    for _ in 0..4 {
        let sibling = world.spawn((
            Transform::identity(),
            GlobalTransform::identity(),
            Name::new("Sibling"),
        ));
        state.pending_reparent = Some((sibling, None));
        process_reparents(&mut world, &mut state);
    }

    let shape = |s: &EditorState| -> Vec<(EntityId, Vec<EntityId>)> {
        s.scene_roots
            .iter()
            .map(|n| (n.entity, n.children.iter().map(|c| c.entity).collect()))
            .collect()
    };

    extract_scene_tree(&world, &mut state);
    let first = shape(&state);
    for _ in 0..8 {
        extract_scene_tree(&world, &mut state);
        assert_eq!(shape(&state), first, "tree shape must not vary per frame");
    }
}

/// An entity whose parent was despawned must still appear, as a root —
/// otherwise it becomes unreachable in the panel.
#[test]
fn extract_scene_tree_surfaces_orphans_as_roots() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();
    let (a, b, _c) = spawn_three_level_chain(&mut world, &mut state);

    world.despawn(a);
    extract_scene_tree(&world, &mut state);

    let roots: Vec<EntityId> = state.scene_roots.iter().map(|n| n.entity).collect();
    assert!(
        roots.contains(&b),
        "B lost its parent and must surface as a root, got {roots:?}"
    );
}

/// Duplication must carry everything the author put on the entity, not a
/// fixed list of component types. `Tag` was the component the old
/// hand-written list forgot; the subtree recipe path covers it — and any
/// component a game crate defines — without naming it.
#[test]
fn duplicate_entity_copies_authored_components() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let source = world.spawn((
        Transform::from_translation(khora_sdk::prelude::math::Vec3::new(4.0, 5.0, 6.0)),
        GlobalTransform::identity(),
        Name::new("Original"),
    ));
    world.add_component(source, Tag::from_iter(["enemy", "spawner"]));

    duplicate_entity(&mut world, source, &mut state);
    let copy = *state
        .selection
        .iter()
        .next()
        .expect("the copy becomes the selection");
    assert_ne!(copy, source, "duplicate must be a distinct entity");

    assert_eq!(
        world.get_component::<Name>(copy).map(|n: &Name| n.as_str()),
        Some("Original (Copy)")
    );
    assert_eq!(
        world
            .get_component::<Transform>(copy)
            .map(|t: &Transform| t.translation),
        world
            .get_component::<Transform>(source)
            .map(|t: &Transform| t.translation),
        "the transform must survive duplication"
    );
    let tag = world
        .get_component::<Tag>(copy)
        .expect("Tag must survive duplication");
    assert!(tag.contains("enemy") && tag.contains("spawner"));
}

/// Duplicating a parent must rebuild its subtree rather than copy the
/// `Children` list verbatim: a cloned list would hold the *source's* entity
/// ids, so the copy would claim the original's children and both would
/// render the same subtree.
#[test]
fn duplicate_entity_rebuilds_the_subtree_without_stealing_children() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let parent = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Root"),
    ));
    let child = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Child"),
    ));
    state.pending_reparent = Some((child, Some(parent)));
    process_reparents(&mut world, &mut state);

    duplicate_entity(&mut world, parent, &mut state);
    let copy = *state
        .selection
        .iter()
        .next()
        .expect("the copy becomes the selection");

    let copied_children = world
        .get_component::<Children>(copy)
        .expect("the copy owns a subtree")
        .0
        .clone();
    assert_eq!(copied_children.len(), 1, "the child must be duplicated too");
    assert_ne!(
        copied_children[0], child,
        "the copy must own a fresh child, not point at the original's"
    );

    let original_children = world.get_component::<Children>(parent).unwrap();
    assert_eq!(
        original_children.0.as_slice(),
        &[child],
        "the original must keep exactly its own child"
    );
}

/// `delete_selection` removes the selected entity from the world and clears
/// the editor's selection/inspector state. A surviving sibling is left
/// untouched.
#[test]
fn delete_selection_removes_selected_entity() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let keep = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Keep"),
    ));
    let drop = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Drop"),
    ));

    state.select(drop);
    delete_selection(&mut world, &mut state);

    let alive: Vec<EntityId> = world.iter_entities().collect();
    assert!(!alive.contains(&drop), "deleted entity must be gone");
    assert!(alive.contains(&keep), "unselected entity must survive");
    assert_eq!(
        world.get_component::<Name>(keep).map(|n| n.as_str()),
        Some("Keep"),
        "survivor's data must be intact"
    );
    assert!(state.selection.is_empty(), "selection cleared after delete");
    assert!(state.inspected.is_none(), "inspector cleared after delete");
}

/// `process_spawns` honours a queued spawn request, creating the entity and
/// selecting it. A simple, dependency-free case ("Empty"/custom tag) keeps
/// the test off the procedural-mesh path.
#[test]
fn process_spawns_creates_and_selects_entity() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let before = world.iter_entities().count();
    state.pending_spawn = Some("Marker".to_owned());
    process_spawns(&mut world, &mut state);

    assert_eq!(
        world.iter_entities().count(),
        before + 1,
        "a spawn request must add exactly one entity"
    );
    let spawned = state
        .single_selected()
        .expect("spawn selects the new entity");
    assert_eq!(
        world.get_component::<Name>(spawned).map(|n| n.as_str()),
        Some("Marker"),
        "the custom request tag becomes the entity Name"
    );
    assert!(state.pending_spawn.is_none(), "the request is consumed");
}

/// Regression: duplicating an entity must carry its material across.
/// A broken implementation drops `MaterialRef`, so the copy renders
/// with no material (logged error) instead of the original look.
#[test]
fn duplicate_entity_clones_material_ref() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let mat = world.add_material(khora_sdk::prelude::materials::StandardMaterial {
        base_color: khora_sdk::prelude::math::LinearRgba::new(0.2, 0.4, 0.6, 1.0),
        roughness: 0.3,
        ..Default::default()
    });
    let original = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Source"),
        mat,
    ));

    duplicate_entity(&mut world, original, &mut state);

    let copy = state.single_selected().expect("duplicate selects the copy");
    assert_ne!(copy, original, "duplicate must produce a new entity");

    let copy_mat = world
        .get_component::<MaterialRef>(copy)
        .expect("copy should carry a MaterialRef");
    match copy_mat {
        MaterialRef::Inline { material, .. } => {
            assert_eq!(
                material.base_color(),
                khora_sdk::prelude::math::LinearRgba::new(0.2, 0.4, 0.6, 1.0),
                "cloned inline material should preserve base color"
            );
        }
        MaterialRef::Asset(_) => panic!("inline material must stay inline after duplication"),
    }
}

/// Regression: duplicating an entity must carry its authored `MeshRef`
/// across so the resolver regenerates the copy's runtime mesh handle.
#[test]
fn duplicate_entity_clones_mesh_ref() {
    let mut world = GameWorld::new();
    let mut state = EditorState::default();

    let original = world.spawn((
        Transform::identity(),
        GlobalTransform::identity(),
        Name::new("Source"),
        MeshRef::procedural(ProceduralMeshKind::Sphere, [0.75, 32.0, 16.0, 0.0]),
    ));

    duplicate_entity(&mut world, original, &mut state);

    let copy = state.single_selected().expect("duplicate selects the copy");
    let copy_mesh = world
        .get_component::<MeshRef>(copy)
        .expect("copy should carry a MeshRef");
    assert_eq!(
        copy_mesh,
        &MeshRef::procedural(ProceduralMeshKind::Sphere, [0.75, 32.0, 16.0, 0.0])
    );
}
