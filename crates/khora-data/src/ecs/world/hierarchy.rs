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
//! the hierarchy a scene load brings in.
//!
//! The hierarchy is stored twice — [`Parent`] on the child and [`Children`]
//! on the parent — so this module is the only one that writes either: an edge
//! at a time through [`World::set_parent`], a whole loaded scene at once
//! through [`LoadedHierarchy`] and [`World::place_loaded_row`].
//!
//! [`Parent`]: crate::ecs::Parent
//! [`Children`]: crate::ecs::Children

use std::any::TypeId;
use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;

use super::World;
use crate::ecs::page::ComponentPage;
use crate::ecs::{Children, Component, Parent};

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
/// A save records each child's `Parent`; each parent's `Children` is derived
/// from those, in the order the save lists the children. Only links between
/// two entities of the same load count: a `Parent` naming anything else is
/// not one.
pub(crate) struct LoadedHierarchy {
    /// Each loaded child, and its loaded parent.
    parents: HashMap<EntityId, EntityId>,
    /// Each loaded parent's children not yet placed with it.
    children: HashMap<EntityId, Vec<EntityId>>,
}

impl LoadedHierarchy {
    /// The hierarchy of `parents` (child → parent, both of the load), each
    /// parent listing its children in `order`. `Err(entity)` when the links
    /// loop through `entity`: a save is input, and a cycle would send every
    /// walk up the hierarchy round forever.
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

/// An entity the links loop through, if they do.
///
/// Each entity is walked once: a walk stops where an earlier one finished, so
/// a long chain costs its length, not its length squared.
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
    /// Re-parents `child`, or detaches it when `new_parent` is `None`.
    ///
    /// The hierarchy is stored twice — [`Parent`] on the child and [`Children`]
    /// on the parent — because both directions are walked every frame. Two
    /// copies of one fact is two chances to disagree, so the only correct place
    /// to write either of them is this module, where both move together: here
    /// for one edge, in [`place_loaded_row`](Self::place_loaded_row) for a
    /// loaded scene.
    ///
    /// Returns `false` when the edge was refused: a missing entity, or a parent
    /// that is already a descendant of `child`. A cycle is refused rather than
    /// created because every consumer of the hierarchy walks it — transform
    /// propagation, the scene tree, recursive despawn — and each of them would
    /// hang on the loop instead of reporting it.
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

        // Detach first, whatever comes next. Skipping this on a re-parent is
        // what leaves a child listed under both its old and its new parent.
        if let Some(former) = self.get::<Parent>(child).map(|p| p.0) {
            if let Some(children) = self.get_mut::<Children>(former) {
                children.0.retain(|listed| *listed != child);
            }
        }

        let Some(parent) = new_parent else {
            // Surgical: drop only `Parent`, keeping the entity's other Spatial
            // components — a detached entity is still in the scene.
            self.remove_component::<Parent>(child).ok();
            return true;
        };

        if let Some(existing) = self.get_mut::<Parent>(child) {
            existing.0 = parent;
        } else {
            self.add_component(child, Parent(parent)).ok();
        }

        if let Some(children) = self.get_mut::<Children>(parent) {
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
    /// Destroying a parent alone would leave its children holding a [`Parent`]
    /// that names a recycled index — the ABA problem the generation counter
    /// exists to catch, reintroduced one level down. A guard holding a weapon as
    /// a child should not leave the weapon floating where it died.
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

        // Detach the root, so its surviving parent's `Children` does not keep an
        // id pointing at nothing.
        self.set_parent(root, None);
        doomed.iter().filter(|e| self.despawn(**e)).count()
    }

    /// Whether `candidate` sits under `ancestor` in the hierarchy.
    pub fn is_descendant_of(&self, candidate: EntityId, ancestor: EntityId) -> bool {
        let mut current = candidate;
        // Every entity on the way up, once: this is the guard against a
        // malformed hierarchy, so it cannot assume the walk ends — and a
        // depth bound would let a long enough chain close into a cycle.
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

    /// A write to a component **by name** — a script command, the editor's
    /// inspector — that lands on the hierarchy, made through this module so
    /// both halves move together: setting or adding `Parent` re-parents
    /// through [`set_parent`](Self::set_parent) (a cycle is refused), removing
    /// it detaches, and `Children` — derived from the children's `Parent` — is
    /// not written by name at all.
    ///
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

    /// Places a loaded row (see [`place_row`](Self::place_row)) with the
    /// hierarchy made to agree as it goes: a `Parent` naming an entity outside
    /// the load is left out of the row, and a parent's `Children` goes into
    /// the same row when the row holds the hierarchy's domain — so a loaded
    /// entity lands in its final page, with no migration to gain its list.
    ///
    /// `fill` pushes the row's other values; a value for a column the row
    /// does not have (a `Parent` left out) is the caller's to skip.
    pub(crate) fn place_loaded_row(
        &mut self,
        entity: EntityId,
        signature: &[TypeId],
        hierarchy: &mut LoadedHierarchy,
        fill: impl FnOnce(&mut ComponentPage),
    ) -> bool {
        let parent_type = TypeId::of::<Parent>();
        let children_type = TypeId::of::<Children>();
        let mut signature = signature.to_vec();
        if !hierarchy.parents.contains_key(&entity) {
            signature.retain(|type_id| *type_id != parent_type);
        }
        let children_domain = self.component_domain(children_type);
        let holds_children_domain = children_domain.is_some()
            && signature
                .iter()
                .any(|type_id| self.component_domain(*type_id) == children_domain);
        let list = if holds_children_domain {
            hierarchy.children.remove(&entity)
        } else {
            None
        };
        if list.is_some() {
            signature.push(children_type);
            signature.sort();
        }
        self.place_row(entity, &signature, |page| {
            fill(page);
            if let (Some(list), Some(column)) = (list, page.columns.get_mut(&children_type)) {
                Children(list).push_into_column(column.as_mut());
            }
        })
    }

    /// Gives every loaded parent no row took its `Children` on its own — a
    /// parent with no page in the hierarchy's domain, whose list is a first
    /// component in that domain, so nothing migrates.
    pub(crate) fn finish_loaded_hierarchy(&mut self, hierarchy: LoadedHierarchy) {
        for (parent, list) in hierarchy.children {
            if let Err(error) = self.add_component(parent, Children(list)) {
                log::error!("a loaded parent could not list its children: {error:?}");
            }
        }
    }
}
