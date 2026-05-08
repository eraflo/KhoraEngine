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
use khora_sdk::khora_data::ecs::{HandleComponent, SemanticDomain};
use khora_sdk::prelude::ecs::*;
use khora_sdk::{GameWorld, Mesh};

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
        } else if world.get_component::<MaterialComponent>(entity).is_some() {
            EntityIcon::Mesh
        } else {
            EntityIcon::Empty
        };

        if let Some(parent) = world.get_component::<Parent>(entity) {
            parent_map.insert(entity, parent.0);
        }

        nodes.insert(
            entity,
            SceneNode {
                entity,
                name,
                icon,
                children: Vec::new(),
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
pub fn process_spawns(world: &mut GameWorld, state: &mut EditorState) {
    if let Some(request) = state.pending_spawn.take() {
        let entity = match request.as_str() {
            "Cube" => khora_sdk::spawn_cube_at(world, khora_sdk::prelude::math::Vec3::ZERO, 1.0)
                .with_component(Name::new("Cube"))
                .build(),
            "Sphere" => khora_sdk::spawn_sphere(world, 0.5, 16, 16)
                .with_component(Name::new("Sphere"))
                .build(),
            "Plane" => khora_sdk::spawn_plane(world, 10.0, 0.0)
                .with_component(Name::new("Plane"))
                .build(),
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
    let mesh_handle = world
        .get_component::<HandleComponent<Mesh>>(entity)
        .cloned();

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
    if let Some(mesh) = mesh_handle {
        world.add_component(new_entity, mesh.clone());
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
