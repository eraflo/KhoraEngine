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

/// Display data for one entity, gathered in the first pass so the tree can be
/// assembled top-down afterwards without touching the `World` again.
struct NodeInfo {
    name: String,
    icon: EntityIcon,
    tag_count: usize,
}

/// Builds the `SceneNode` for `entity` and, recursively, its children.
///
/// `visited` guards against a malformed `Parent` chain looping back on itself:
/// a cycle would otherwise recurse until the stack blew. Returns `None` for an
/// entity already placed in the tree, which is what breaks the loop.
fn build_scene_node(
    entity: EntityId,
    info: &std::collections::HashMap<EntityId, NodeInfo>,
    children_of: &std::collections::HashMap<EntityId, Vec<EntityId>>,
    visited: &mut std::collections::HashSet<EntityId>,
) -> Option<SceneNode> {
    if !visited.insert(entity) {
        return None;
    }
    let node_info = info.get(&entity)?;
    let children = children_of
        .get(&entity)
        .map(|ids| {
            ids.iter()
                .filter_map(|&child| build_scene_node(child, info, children_of, visited))
                .collect()
        })
        .unwrap_or_default();

    Some(SceneNode {
        entity,
        name: node_info.name.clone(),
        icon: node_info.icon,
        children,
        tag_count: node_info.tag_count,
    })
}

/// Extracts a scene tree snapshot from the ECS world into editor state.
///
/// Assembles the tree **top-down from the roots**, in `entity.index` order at
/// every level. The previous bottom-up pass folded each child into its parent
/// by draining a `HashMap`, which made the result depend on iteration order —
/// and `HashMap` re-seeds its hasher per instance, so the order differed every
/// frame. Two symptoms followed: a grandchild whose parent had already been
/// moved was re-inserted as a root (so three-level hierarchies lost a level at
/// random), and siblings reordered continuously, which let a click land on a
/// different entity than the one aimed at.
pub fn extract_scene_tree(world: &GameWorld, state: &mut EditorState) {
    let entities: Vec<EntityId> = world.iter_entities().collect();
    state.entity_count = entities.len();

    let live: std::collections::HashSet<EntityId> = entities.iter().copied().collect();
    let mut info: std::collections::HashMap<EntityId, NodeInfo> =
        std::collections::HashMap::with_capacity(entities.len());
    let mut children_of: std::collections::HashMap<EntityId, Vec<EntityId>> =
        std::collections::HashMap::new();
    let mut parent_of: std::collections::HashMap<EntityId, EntityId> =
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

        // Trust `Parent` rather than the parent's `Children` list: `Parent` is
        // the authored edge, `Children` only its derived inverse index.
        if let Some(parent) = world.get_component::<Parent>(entity) {
            let parent = parent.0;
            if live.contains(&parent) && parent != entity {
                parent_of.insert(entity, parent);
                children_of.entry(parent).or_default().push(entity);
            }
        }

        let tag_count = world
            .get_component::<Tag>(entity)
            .map(|t| t.len())
            .unwrap_or(0);

        info.insert(
            entity,
            NodeInfo {
                name,
                icon,
                tag_count,
            },
        );
    }

    for siblings in children_of.values_mut() {
        siblings.sort_unstable_by_key(|e| e.index);
    }

    // An entity is a root when it has no parent, or when its parent was
    // despawned — an orphan must still be reachable in the panel.
    let mut roots: Vec<EntityId> = entities
        .iter()
        .copied()
        .filter(|e| !parent_of.contains_key(e))
        .collect();
    roots.sort_unstable_by_key(|e| e.index);

    let mut visited = std::collections::HashSet::with_capacity(entities.len());
    state.scene_roots = roots
        .into_iter()
        .filter_map(|root| build_scene_node(root, &info, &children_of, &mut visited))
        .collect();
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

/// Spawns an entity that references a mesh asset by UUID at a world point,
/// attaching a default surface material so it renders immediately. The
/// `asset_resolver_system` loads the mesh from the `MeshRef::Asset` next tick.
/// Returns the new entity. `label` is used to derive a readable `Name`.
pub fn spawn_mesh_asset(
    world: &mut GameWorld,
    uuid: khora_sdk::khora_core::asset::AssetUUID,
    point: [f32; 3],
    label: &str,
) -> EntityId {
    let mat = world.add_material(default_surface_material());
    let name = std::path::Path::new(label)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(label)
        .to_string();
    let entity = world.spawn((
        Transform::from_translation(khora_sdk::prelude::math::Vec3::new(
            point[0], point[1], point[2],
        )),
        GlobalTransform::identity(),
        Name::new(name),
        MeshRef::Asset(uuid),
    ));
    world.add_component(entity, mat);
    entity
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

/// Duplicates an entity and everything below it.
///
/// Goes through the same subtree recipe round-trip as "Save as Prefab" rather
/// than copying a hand-listed set of components, so the copy carries `Tag`,
/// every user-defined component and the whole descendant subtree — none of
/// which a fixed list could know about. `serialize_all_components` filters out
/// engine-written components, so `GlobalTransform` and `Children` are rebuilt
/// for the copy instead of being cloned with the original's entity ids.
///
/// `serialize_subtree` deliberately drops the root's own parent edge (a
/// `.kprefab` has to be self-contained), so the copy is re-parented here to
/// land as a sibling of the original.
pub fn duplicate_entity(world: &mut GameWorld, entity: EntityId, state: &mut EditorState) {
    let recipe = match khora_sdk::serialize_subtree(world.inner_world(), entity) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("Duplicate failed: could not read {entity:?}: {e}");
            return;
        }
    };

    let original_parent = world.get_component::<Parent>(entity).map(|p: &Parent| p.0);
    let copy_name = world
        .get_component::<Name>(entity)
        .map(|n: &Name| format!("{} (Copy)", n.as_str()))
        .unwrap_or_else(|| "Copy".to_owned());

    let new_entity = match khora_sdk::instantiate_subtree(world.inner_world_mut(), &recipe) {
        Ok(id) => id,
        Err(e) => {
            log::error!("Duplicate failed: could not rebuild {entity:?}: {e}");
            return;
        }
    };

    if let Some(name) = world.get_component_mut::<Name>(new_entity) {
        *name = Name::new(copy_name);
    } else {
        world.add_component(new_entity, Name::new(copy_name));
    }
    if let Some(parent) = original_parent {
        world.set_parent(new_entity, Some(parent));
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
}
