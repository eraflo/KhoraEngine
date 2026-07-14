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

//! Pure ECS operations used by the editor application.

use khora_sdk::editor_ui::*;
use khora_sdk::khora_data::ecs::{SemanticDomain, Tag};
use khora_sdk::prelude::ecs::*;
use khora_sdk::GameWorld;

/// Maps [`SemanticDomain`] to the small integer tag the editor side uses
/// in [`ComponentJson::domain`]. Kept here so `khora-core` doesn't have to
/// know about `khora-data`'s domain enum — the inspector reads the tag and
/// dispatches to category labels.
fn domain_tag(d: SemanticDomain) -> u8 {
    match d {
        SemanticDomain::Spatial => 0,
        SemanticDomain::Render => 1,
        SemanticDomain::Audio => 2,
        SemanticDomain::Physics => 3,
        SemanticDomain::Ui => 4,
    }
}

/// Extracts a scene tree snapshot from the ECS world into editor state.
pub fn extract_scene_tree(world: &GameWorld, state: &mut EditorState) {
    let entities: Vec<EntityId> = world.iter_entities().collect();
    state.entity_count = entities.len();

    let mut nodes: std::collections::HashMap<EntityId, SceneNode> =
        std::collections::HashMap::new();
    let mut parent_map: std::collections::HashMap<EntityId, EntityId> =
        std::collections::HashMap::new();

    for &entity in &entities {
        let name = world
            .get_component::<Name>(entity)
            .map(|n: &Name| n.as_str().to_owned())
            .unwrap_or_else(|| format!("Entity {}", entity.index));

        let icon = if world.get_component::<Camera>(entity).is_some() {
            EntityIcon::Camera
        } else if world.get_component::<Light>(entity).is_some() {
            EntityIcon::Light
        } else if world.get_component::<AudioSource>(entity).is_some() {
            EntityIcon::Audio
        } else if world.get_component::<MeshRef>(entity).is_some() {
            EntityIcon::Mesh
        } else {
            EntityIcon::Empty
        };

        if let Some(parent) = world.get_component::<Parent>(entity) {
            parent_map.insert(entity, parent.0);
        }

        let tag_count = world
            .get_component::<Tag>(entity)
            .map(|t| t.len())
            .unwrap_or(0);

        nodes.insert(
            entity,
            SceneNode {
                entity,
                name,
                icon,
                children: Vec::new(),
                tag_count,
            },
        );
    }

    let child_parent_pairs: Vec<(EntityId, EntityId)> =
        parent_map.iter().map(|(&c, &p)| (c, p)).collect();

    for (child_id, parent_id) in &child_parent_pairs {
        if let Some(child_node) = nodes.remove(child_id) {
            if let Some(parent_node) = nodes.get_mut(parent_id) {
                parent_node.children.push(child_node);
            } else {
                // Parent not found: keep as root.
                nodes.insert(*child_id, child_node);
            }
        }
    }

    let mut roots: Vec<SceneNode> = nodes.into_values().collect();
    roots.sort_by_key(|n| n.entity.index);

    state.scene_roots = roots;
}

/// Processes pending spawn requests from the scene tree panel.
/// The material the editor attaches to a freshly-spawned primitive so it is
/// immediately visible and editable. The engine projection has no implicit
/// default material (a meshed entity without one is a clear, logged error),
/// so the authoring tool supplies an explicit one — a neutral matte grey the
/// user then tweaks in the inspector.
fn default_surface_material() -> khora_sdk::prelude::materials::StandardMaterial {
    khora_sdk::prelude::materials::StandardMaterial {
        base_color: khora_sdk::prelude::math::LinearRgba::new(0.7, 0.7, 0.7, 1.0),
        roughness: 0.8,
        ..Default::default()
    }
}

pub fn process_spawns(world: &mut GameWorld, state: &mut EditorState) {
    if let Some(request) = state.pending_spawn.take() {
        let entity = match request.as_str() {
            "Cube" => {
                let mat = world.add_material(default_surface_material());
                khora_sdk::spawn_cube_at(world, khora_sdk::prelude::math::Vec3::ZERO, 1.0)
                    .with_component(Name::new("Cube"))
                    .with_component(mat)
                    .build()
            }
            "Sphere" => {
                let mat = world.add_material(default_surface_material());
                khora_sdk::spawn_sphere(world, 0.5, 16, 16)
                    .with_component(Name::new("Sphere"))
                    .with_component(mat)
                    .build()
            }
            "Plane" => {
                let mat = world.add_material(default_surface_material());
                khora_sdk::spawn_plane(world, 10.0, 0.0)
                    .with_component(Name::new("Plane"))
                    .with_component(mat)
                    .build()
            }
            "Light" => world.spawn((
                Transform::identity(),
                GlobalTransform::identity(),
                Name::new("Light"),
                Light::directional(),
            )),
            "Camera" => {
                let cam =
                    Camera::new_perspective(std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0);
                world.spawn((
                    Transform::identity(),
                    GlobalTransform::identity(),
                    Name::new("Camera"),
                    cam,
                ))
            }
            _ => world.spawn((
                Transform::identity(),
                GlobalTransform::identity(),
                Name::new(&request),
            )),
        };

        state.select(entity);
        log::info!("Spawned entity {:?} ({})", entity, request);
    }
}

/// Drains `state.pending_reparent` and applies it to the ECS hierarchy.
///
/// Set by `scene_tree`'s drag-and-drop handler. The actual cycle check and
/// `Parent`/`Children` bookkeeping live in `GameWorld::set_parent`.
pub fn process_reparents(world: &mut GameWorld, state: &mut EditorState) {
    if let Some((child, new_parent)) = state.pending_reparent.take() {
        world.set_parent(child, new_parent);
        log::info!(
            "Reparented {:?} → {:?}",
            child,
            new_parent
                .map(|p| format!("{:?}", p))
                .unwrap_or_else(|| "<root>".to_owned())
        );
    }
}

/// Duplicates one entity by cloning known components.
pub fn duplicate_entity(world: &mut GameWorld, entity: EntityId, state: &mut EditorState) {
    let name = world
        .get_component::<Name>(entity)
        .map(|n: &Name| format!("{} (Copy)", n.as_str()));
    let transform = world.get_component::<Transform>(entity).copied();
    let camera = world.get_component::<Camera>(entity).cloned();
    let light = world.get_component::<Light>(entity).cloned();
    let rigid_body = world.get_component::<RigidBody>(entity).cloned();
    let collider = world.get_component::<Collider>(entity).cloned();
    let audio_source = world.get_component::<AudioSource>(entity).cloned();
    let mesh_ref = world.get_component::<MeshRef>(entity).cloned();
    let material_ref = world.get_component::<MaterialRef>(entity).cloned();

    let new_entity = world.spawn((
        transform.unwrap_or_else(Transform::identity),
        GlobalTransform::identity(),
        Name::new(name.unwrap_or_else(|| "Copy".to_owned())),
    ));

    if let Some(cam) = camera {
        world.add_component(new_entity, cam);
    }
    if let Some(light) = light {
        world.add_component(new_entity, light);
    }
    if let Some(rb) = rigid_body {
        world.add_component(new_entity, rb);
    }
    if let Some(col) = collider {
        world.add_component(new_entity, col);
    }
    if let Some(audio) = audio_source {
        world.add_component(new_entity, audio);
    }
    if let Some(mesh) = mesh_ref {
        world.add_component(new_entity, mesh);
    }
    if let Some(material) = material_ref {
        world.add_component(new_entity, material);
    }

    state.select(new_entity);
    log::info!("Duplicated entity {:?} -> {:?}", entity, new_entity);
}

/// Deletes every currently selected entity and clears selection/inspector state.
pub fn delete_selection(world: &mut GameWorld, state: &mut EditorState) {
    let to_delete: Vec<EntityId> = state.selection.iter().copied().collect();
    for entity in &to_delete {
        world.despawn(*entity);
    }
    if !to_delete.is_empty() {
        log::info!("Deleted {} entities", to_delete.len());
    }
    state.clear_selection();
    state.inspected = None;
}

/// Keeps ECS scene-camera activation consistent with the current play mode.
pub fn sync_scene_cameras_for_mode(world: &mut GameWorld, mode: PlayMode) {
    let entities: Vec<EntityId> = world.iter_entities().collect();
    let camera_states: Vec<(EntityId, bool)> = entities
        .iter()
        .filter_map(|&entity| {
            world
                .get_component::<Camera>(entity)
                .map(|cam| (entity, cam.is_active))
        })
        .collect();

    match mode {
        // Editing mode always uses the dedicated editor camera.
        PlayMode::Editing => {
            for (entity, _) in camera_states {
                if let Some(cam) = world.get_component_mut::<Camera>(entity) {
                    cam.is_active = false;
                }
            }
        }
        // During play/pause, ensure at least one scene camera is active.
        PlayMode::Playing | PlayMode::Paused => {
            if camera_states.iter().any(|(_, is_active)| *is_active) {
                return;
            }
            if let Some((entity, _)) = camera_states.first().copied() {
                if let Some(cam) = world.get_component_mut::<Camera>(entity) {
                    cam.is_active = true;
                }
            }
        }
    }
}

/// Extracts inspectable component snapshots for the single selected entity.
pub fn extract_inspected(world: &GameWorld, state: &mut EditorState) {
    let entity = match state.single_selected() {
        Some(entity) => entity,
        None => {
            state.inspected = None;
            return;
        }
    };
    // An entity is selected — drop any prior asset selection so the
    // Inspector switches out of asset-metadata mode.
    state.inspected_asset_path = None;

    let name = world
        .get_component::<Name>(entity)
        .map(|n: &Name| n.as_str().to_owned())
        .unwrap_or_else(|| format!("Entity {}", entity.index));

    // Collect every component on this entity, captured generically as
    // JSON via the macro-generated `to_json`. The inspector walks this
    // list and renders every entry through a single field-typed walker
    // — adding a new ECS component costs zero editor code.
    //
    // We also populate the global `component_domain_registry` with the
    // domain of every registered type (regardless of whether this entity
    // has it) so the "+ Add Component" menu can categorise candidates
    // without re-querying the world.
    let inner_world = world.inner_world();
    let mut components_json = Vec::new();
    state.component_domain_registry.clear();
    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        let domain = inner_world.component_domain(reg.type_id).map(domain_tag);
        if let Some(tag) = domain {
            state
                .component_domain_registry
                .insert(reg.type_name.to_string(), tag);
        }

        let Some(value) = (reg.to_json)(inner_world, entity) else {
            continue;
        };
        components_json.push(ComponentJson {
            type_name: reg.type_name.to_string(),
            domain,
            value,
        });
    }

    state.inspected = Some(InspectedEntity {
        entity,
        name,
        components_json,
    });
}

/// Applies queued property edits back into ECS components.
///
/// All component edits go through one path: the inspector ships a JSON
/// patch and we look up the component's `from_json` from the inventory
/// registration. The single special case is `Name`, which lives in the
/// inspector header (not as a component card) and so has its own variant.
pub fn apply_edits(world: &mut GameWorld, state: &mut EditorState) {
    let edits = state.drain_edits();
    for edit in edits {
        match edit {
            PropertyEdit::SetName(entity, new_name) => {
                if let Some(name) = world.get_component_mut::<Name>(entity) {
                    *name = Name::new(new_name);
                }
            }
            PropertyEdit::SetComponentJson {
                entity,
                type_name,
                value,
            } => {
                let inner = world.inner_world_mut();
                let mut applied = false;
                for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
                    if reg.type_name == type_name {
                        match (reg.from_json)(inner, entity, &value) {
                            Ok(()) => applied = true,
                            Err(e) => {
                                log::warn!("Failed to apply JSON edit to {}: {}", type_name, e)
                            }
                        }
                        break;
                    }
                }
                if !applied {
                    log::warn!("No registration found for component '{}'", type_name);
                }
            }
            PropertyEdit::RemoveComponent { entity, type_name } => {
                let inner = world.inner_world_mut();
                let mut applied = false;
                for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
                    if reg.type_name == type_name {
                        match (reg.remove)(inner, entity) {
                            Ok(()) => applied = true,
                            Err(e) => log::warn!("Failed to remove component {}: {}", type_name, e),
                        }
                        break;
                    }
                }
                if !applied {
                    log::warn!("No registration found for component '{}'", type_name);
                }
            }
        }
    }
}

/// Adds a new component to an existing entity by dispatching through the inventory registry.
pub fn add_component_to_entity(world: &mut GameWorld, entity: EntityId, type_name: &str) {
    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        if reg.type_name == type_name {
            if let Err(e) = (reg.create_default)(world.inner_world_mut(), entity) {
                log::error!("Failed to add component {}: {}", type_name, e);
            } else {
                log::info!("Added {} component to entity {:?}", type_name, entity);
            }
            return;
        }
    }
    log::warn!("No component registration found for type '{}'", type_name);
}

#[cfg(test)]
mod tests {
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
}
