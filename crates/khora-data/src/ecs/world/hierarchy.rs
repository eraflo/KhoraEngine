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

//! Parent/child links: reparenting, subtree despawn, ancestry checks, and
//! the hierarchy a scene load brings in. The only writer of [`Parent`] and
//! [`Children`], the two copies of one fact.
//!
//! [`Parent`]: crate::ecs::Parent
//! [`Children`]: crate::ecs::Children

use std::any::TypeId;
use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;

use super::World;
use crate::ecs::page::ComponentPage;
use crate::ecs::{Children, Component, ComponentKey, Parent};

/// What a by-name write does to a component — see
/// [`World::write_hierarchy_by_name`].
#[derive(Debug, Clone, Copy)]
pub enum HierarchyWrite<'v> {
    /// Gives the component this value (an add, or a write over it).
    Set(&'v serde_json::Value),
    /// Takes the component away.
    Remove,
}

/// The hierarchy a scene load brings in, checked before anything is placed.
///
/// Each parent's `Children` is derived from the saved `Parent`s, in save
/// order. Only links between two entities of the same load count.
pub(crate) struct LoadedHierarchy {
    /// Each loaded child, and its loaded parent.
    parents: HashMap<EntityId, EntityId>,
    /// Each loaded parent's children not yet placed with it.
    children: HashMap<EntityId, Vec<EntityId>>,
}

impl LoadedHierarchy {
    /// Builds the hierarchy of `parents` (child → parent), each parent listing
    /// its children in `order`.
    ///
    /// `Err(entity)` when the links loop through `entity`: a save is input,
    /// and a cycle would hang every walk up the hierarchy.
    pub(crate) fn new(
        parents: HashMap<EntityId, EntityId>,
        order: impl IntoIterator<Item = EntityId>,
    ) -> Result<Self, EntityId> {
        if let Some(looping) = cycle_in(&parents) {
            return Err(looping);
        }
        let mut children: HashMap<EntityId, Vec<EntityId>> = HashMap::new();
        for entity in order {
            if let Some(parent) = parents.get(&entity) {
                children.entry(*parent).or_default().push(entity);
            }
        }
        Ok(Self { parents, children })
    }
}

/// Returns an entity the links loop through, if they do, walking each entity once.
fn cycle_in(parents: &HashMap<EntityId, EntityId>) -> Option<EntityId> {
    // `false` while on the walk in progress, `true` once known to reach a root.
    let mut state: HashMap<EntityId, bool> = HashMap::with_capacity(parents.len());
    let mut path = Vec::new();
    for &start in parents.keys() {
        let mut at = start;
        loop {
            match state.get(&at) {
                Some(true) => break,
                Some(false) => return Some(at),
                None => {}
            }
            state.insert(at, false);
            path.push(at);
            match parents.get(&at) {
                Some(&up) => at = up,
                None => break,
            }
        }
        for entity in path.drain(..) {
            state.insert(entity, true);
        }
    }
    None
}

impl World {
    /// Moves `child` under `new_parent`, or detaches it when `new_parent` is `None`.
    ///
    /// Moves [`Parent`] on the child and [`Children`] on the parent together.
    /// Returns `false` when the edge is refused: a missing entity, or a parent
    /// already below `child`, since every walk of the hierarchy would hang on
    /// the cycle.
    ///
    /// [`Parent`]: crate::ecs::Parent
    /// [`Children`]: crate::ecs::Children
    pub fn set_parent(&mut self, child: EntityId, new_parent: Option<EntityId>) -> bool {
        if !self.contains(child) {
            return false;
        }
        if let Some(parent) = new_parent {
            if !self.contains(parent) || parent == child || self.is_descendant_of(parent, child) {
                return false;
            }
        }

        // Detached first, even on a re-parent, or the child stays listed
        // under its old parent too.
        if let Some(former) = self.get::<Parent>(child).map(|p| p.0) {
            if let Some(mut children) = self.get_mut::<Children>(former) {
                children.0.retain(|listed| *listed != child);
            }
        }

        let Some(parent) = new_parent else {
            // Surgical: drop only `Parent`, keeping the entity's other Spatial
            // components — a detached entity is still in the scene.
            self.remove_component::<Parent>(child).ok();
            return true;
        };

        if let Some(mut existing) = self.get_mut::<Parent>(child) {
            existing.0 = parent;
        } else {
            self.add_component(child, Parent(parent)).ok();
        }

        if let Some(mut children) = self.get_mut::<Children>(parent) {
            if !children.0.contains(&child) {
                children.0.push(child);
            }
        } else {
            self.add_component(parent, Children(vec![child])).ok();
        }
        true
    }

    /// Despawns `root` and every entity beneath it, returning how many went.
    ///
    /// Despawning a parent alone would leave its children's [`Parent`] naming
    /// an index that will be recycled.
    ///
    /// [`Parent`]: crate::ecs::Parent
    pub fn despawn_subtree(&mut self, root: EntityId) -> usize {
        if !self.contains(root) {
            return 0;
        }

        // Collected before anything is removed: reading the hierarchy while
        // dismantling it would miss half of it.
        let mut doomed = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut frontier = vec![root];
        while let Some(entity) = frontier.pop() {
            // `set_parent` refuses cycles, but a scene loaded from a corrupted
            // file has not been through it — and a frozen frame is a worse
            // failure than a wrong one.
            if !seen.insert(entity) {
                continue;
            }
            doomed.push(entity);
            if let Some(children) = self.get::<Children>(entity) {
                frontier.extend(children.0.iter().copied());
            }
        }

        // The root's surviving parent must not keep it in `Children`.
        self.set_parent(root, None);
        doomed.iter().filter(|e| self.despawn(**e)).count()
    }

    /// Returns whether `candidate` sits under `ancestor` in the hierarchy.
    pub fn is_descendant_of(&self, candidate: EntityId, ancestor: EntityId) -> bool {
        let mut current = candidate;
        // This is the guard against a malformed hierarchy, so it cannot
        // assume the walk ends; a depth bound would miss a long enough cycle.
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current) {
            let Some(parent) = self.get::<Parent>(current) else {
                return false;
            };
            if parent.0 == ancestor {
                return true;
            }
            current = parent.0;
        }
        log::warn!("is_descendant_of: the hierarchy above {candidate:?} loops");
        false
    }

    /// Applies a by-name write of `Parent` through [`set_parent`](Self::set_parent).
    ///
    /// Removing `Parent` detaches; `Children` is derived and refused by name.
    /// `None` when `type_name` is not part of the hierarchy: the caller writes
    /// it as any other component.
    pub fn write_hierarchy_by_name(
        &mut self,
        entity: EntityId,
        type_name: &str,
        write: HierarchyWrite<'_>,
    ) -> Option<Result<(), String>> {
        match type_name {
            "Parent" => Some(match write {
                HierarchyWrite::Remove => {
                    self.set_parent(entity, None);
                    Ok(())
                }
                HierarchyWrite::Set(json) => {
                    match serde_json::from_value::<crate::ecs::SerializableParent>(json.clone()) {
                        Ok(parent) => {
                            let parent = Parent::from(parent).0;
                            if self.set_parent(entity, Some(parent)) {
                                Ok(())
                            } else {
                                Err(format!(
                                    "{entity:?} cannot be parented to {parent:?}: a missing \
                                     entity, or one already below it"
                                ))
                            }
                        }
                        Err(error) => Err(format!("not a parent: {error}")),
                    }
                }
            }),
            "Children" => Some(Err(
                "`Children` is derived from each child's `Parent`; re-parent the child instead"
                    .to_owned(),
            )),
            _ => None,
        }
    }

    /// Places a loaded row as [`place_row`](Self::place_row) does, with its
    /// hierarchy made to agree.
    ///
    /// A `Parent` naming an entity outside the load is left out; a parent's
    /// `Children` joins the row when it holds the hierarchy's domain, so no
    /// migration follows. `fill` skips values for columns the row lacks.
    pub(crate) fn place_loaded_row(
        &mut self,
        entity: EntityId,
        signature: &[ComponentKey],
        hierarchy: &mut LoadedHierarchy,
        fill: impl FnOnce(&mut ComponentPage),
    ) -> bool {
        let parent_key = ComponentKey::of::<Parent>();
        let children_type = TypeId::of::<Children>();
        let children_key = ComponentKey::Rust(children_type);
        let mut keys = signature.to_vec();
        if !hierarchy.parents.contains_key(&entity) {
            keys.retain(|key| *key != parent_key);
        }
        let children_domain = self.component_domain(children_type);
        let registry = &self.storage.registry;
        let holds_children_domain = children_domain.is_some()
            && keys
                .iter()
                .any(|key| registry.domain_of(*key) == children_domain);
        let list = if holds_children_domain {
            hierarchy.children.remove(&entity)
        } else {
            None
        };
        if list.is_some() {
            keys.push(children_key);
            keys.sort();
        }
        self.place_row(entity, &keys, |page| {
            fill(page);
            if let (Some(list), Some(column)) = (
                list,
                page.columns.get_mut(&ComponentKey::Rust(children_type)),
            ) {
                Children(list).push_into_column(column.as_mut());
            }
        })
    }

    /// Adds `Children` to each loaded parent whose row did not take it.
    ///
    /// Such a parent has no component in the hierarchy's domain, so nothing migrates.
    pub(crate) fn finish_loaded_hierarchy(&mut self, hierarchy: LoadedHierarchy) {
        for (parent, list) in hierarchy.children {
            if let Err(error) = self.add_component(parent, Children(list)) {
                log::error!("a loaded parent could not list its children: {error:?}");
            }
        }
    }
}
