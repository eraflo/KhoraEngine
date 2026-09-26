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

//! Building the editor's scene tree from the world.

use khora_sdk::editor_ui::*;
use khora_sdk::khora_data::ecs::{SemanticDomain, Tag};
use khora_sdk::prelude::ecs::*;
use khora_sdk::GameWorld;

/// Maps [`SemanticDomain`] to the small integer tag the editor side uses
/// in [`ComponentJson::domain`]. Kept here so `khora-core` doesn't have to
/// know about `khora-data`'s domain enum — the inspector reads the tag and
/// dispatches to category labels.
///
/// The numbering is [`SemanticDomain::index`] rather than a table of its own:
/// that index already has to be dense and stable to address the `World`'s
/// per-domain arrays, and a second listing would only be somewhere for the two
/// to disagree.
pub(super) fn domain_tag(d: SemanticDomain) -> u8 {
    d.index() as u8
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
