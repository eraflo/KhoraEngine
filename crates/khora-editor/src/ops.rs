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

use khora_core::ui::editor::*;
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::*;
use khora_sdk::GameWorld;

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

    let name = world
        .get_component::<Name>(entity)
        .map(|n: &Name| n.as_str().to_owned())
        .unwrap_or_else(|| format!("Entity {}", entity.index));

    let transform = world
        .get_component::<Transform>(entity)
        .map(|t| TransformSnapshot {
            translation: t.translation,
            rotation: t.rotation,
            scale: t.scale,
        });

    let camera = world.get_component::<Camera>(entity).map(|c| {
        let (projection_index, fov_y_radians, ortho_width, ortho_height) = match c.projection {
            ProjectionType::Perspective { fov_y_radians } => (0, fov_y_radians, 10.0, 10.0),
            ProjectionType::Orthographic { width, height } => {
                (1, std::f32::consts::FRAC_PI_4, width, height)
            }
        };
        CameraSnapshot {
            projection_index,
            fov_y_radians,
            ortho_width,
            ortho_height,
            aspect_ratio: c.aspect_ratio,
            z_near: c.z_near,
            z_far: c.z_far,
            is_active: c.is_active,
        }
    });

    let light = world
        .get_component::<Light>(entity)
        .map(|l| match &l.light_type {
            LightType::Directional(d) => LightSnapshot {
                light_kind: 0,
                direction: d.direction,
                color: d.color,
                intensity: d.intensity,
                range: 0.0,
                inner_cone_angle: 0.0,
                outer_cone_angle: 0.0,
                shadow_enabled: d.shadow_enabled,
                shadow_bias: d.shadow_bias,
                shadow_normal_bias: d.shadow_normal_bias,
                enabled: l.enabled,
            },
            LightType::Point(p) => LightSnapshot {
                light_kind: 1,
                direction: khora_sdk::prelude::math::Vec3::ZERO,
                color: p.color,
                intensity: p.intensity,
                range: p.range,
                inner_cone_angle: 0.0,
                outer_cone_angle: 0.0,
                shadow_enabled: p.shadow_enabled,
                shadow_bias: p.shadow_bias,
                shadow_normal_bias: p.shadow_normal_bias,
                enabled: l.enabled,
            },
            LightType::Spot(s) => LightSnapshot {
                light_kind: 2,
                direction: s.direction,
                color: s.color,
                intensity: s.intensity,
                range: s.range,
                inner_cone_angle: s.inner_cone_angle,
                outer_cone_angle: s.outer_cone_angle,
                shadow_enabled: s.shadow_enabled,
                shadow_bias: s.shadow_bias,
                shadow_normal_bias: s.shadow_normal_bias,
                enabled: l.enabled,
            },
        });

    let rigid_body = world.get_component::<RigidBody>(entity).map(|rb| {
        let body_type_index = match rb.body_type {
            BodyType::Dynamic => 0,
            BodyType::Static => 1,
            BodyType::Kinematic => 2,
        };
        RigidBodySnapshot {
            body_type_index,
            mass: rb.mass,
            ccd_enabled: rb.ccd_enabled,
            linear_velocity: rb.linear_velocity,
            angular_velocity: rb.angular_velocity,
        }
    });

    let collider = world.get_component::<Collider>(entity).map(|col| {
        let (shape_index, box_half_extents, sphere_radius, capsule_radius, capsule_half_height) =
            match col.shape {
                ColliderShape::Box(he) => (0, he, 0.5, 0.5, 0.5),
                ColliderShape::Sphere(r) => (1, khora_sdk::prelude::math::Vec3::ONE, r, 0.5, 0.5),
                ColliderShape::Capsule(half_h, r) => {
                    (2, khora_sdk::prelude::math::Vec3::ONE, 0.5, r, half_h)
                }
            };
        ColliderSnapshot {
            shape_index,
            box_half_extents,
            sphere_radius,
            capsule_radius,
            capsule_half_height,
            friction: col.friction,
            restitution: col.restitution,
            is_sensor: col.is_sensor,
        }
    });

    let audio_source = world
        .get_component::<AudioSource>(entity)
        .map(|a| AudioSourceSnapshot {
            volume: a.volume,
            looping: a.looping,
            autoplay: a.autoplay,
        });

    state.inspected = Some(InspectedEntity {
        entity,
        name,
        transform,
        camera,
        light,
        rigid_body,
        collider,
        audio_source,
    });
}

/// Applies queued property edits back into ECS components.
pub fn apply_edits(world: &mut GameWorld, state: &mut EditorState) {
    let edits = state.drain_edits();
    for edit in edits {
        match edit {
            PropertyEdit::SetName(entity, new_name) => {
                if let Some(name) = world.get_component_mut::<Name>(entity) {
                    *name = Name::new(new_name);
                }
            }
            PropertyEdit::SetTransform(entity, snap) => {
                if let Some(t) = world.get_component_mut::<Transform>(entity) {
                    t.translation = snap.translation;
                    t.rotation = snap.rotation;
                    t.scale = snap.scale;
                }
            }
            PropertyEdit::SetCamera(entity, snap) => {
                if let Some(c) = world.get_component_mut::<Camera>(entity) {
                    c.projection = match snap.projection_index {
                        0 => ProjectionType::Perspective {
                            fov_y_radians: snap.fov_y_radians,
                        },
                        _ => ProjectionType::Orthographic {
                            width: snap.ortho_width,
                            height: snap.ortho_height,
                        },
                    };
                    c.aspect_ratio = snap.aspect_ratio;
                    c.z_near = snap.z_near;
                    c.z_far = snap.z_far;
                    c.is_active = snap.is_active;
                }
            }
            PropertyEdit::SetLight(entity, snap) => {
                if let Some(l) = world.get_component_mut::<Light>(entity) {
                    l.enabled = snap.enabled;
                    l.light_type = match snap.light_kind {
                        0 => LightType::Directional(DirectionalLight {
                            direction: snap.direction,
                            color: snap.color,
                            intensity: snap.intensity,
                            shadow_enabled: snap.shadow_enabled,
                            shadow_bias: snap.shadow_bias,
                            shadow_normal_bias: snap.shadow_normal_bias,
                        }),
                        1 => LightType::Point(PointLight {
                            color: snap.color,
                            intensity: snap.intensity,
                            range: snap.range,
                            shadow_enabled: snap.shadow_enabled,
                            shadow_bias: snap.shadow_bias,
                            shadow_normal_bias: snap.shadow_normal_bias,
                        }),
                        _ => LightType::Spot(SpotLight {
                            direction: snap.direction,
                            color: snap.color,
                            intensity: snap.intensity,
                            range: snap.range,
                            inner_cone_angle: snap.inner_cone_angle,
                            outer_cone_angle: snap.outer_cone_angle,
                            shadow_enabled: snap.shadow_enabled,
                            shadow_bias: snap.shadow_bias,
                            shadow_normal_bias: snap.shadow_normal_bias,
                        }),
                    };
                }
            }
            PropertyEdit::SetRigidBody(entity, snap) => {
                if let Some(rb) = world.get_component_mut::<RigidBody>(entity) {
                    rb.body_type = match snap.body_type_index {
                        0 => BodyType::Dynamic,
                        1 => BodyType::Static,
                        _ => BodyType::Kinematic,
                    };
                    rb.mass = snap.mass;
                    rb.ccd_enabled = snap.ccd_enabled;
                    rb.linear_velocity = snap.linear_velocity;
                    rb.angular_velocity = snap.angular_velocity;
                }
            }
            PropertyEdit::SetCollider(entity, snap) => {
                if let Some(col) = world.get_component_mut::<Collider>(entity) {
                    col.shape = match snap.shape_index {
                        0 => ColliderShape::Box(snap.box_half_extents),
                        1 => ColliderShape::Sphere(snap.sphere_radius),
                        _ => ColliderShape::Capsule(snap.capsule_half_height, snap.capsule_radius),
                    };
                    col.friction = snap.friction;
                    col.restitution = snap.restitution;
                    col.is_sensor = snap.is_sensor;
                }
            }
            PropertyEdit::SetAudioSource(entity, snap) => {
                if let Some(a) = world.get_component_mut::<AudioSource>(entity) {
                    a.volume = snap.volume;
                    a.looping = snap.looping;
                    a.autoplay = snap.autoplay;
                }
            }
        }
    }
}
