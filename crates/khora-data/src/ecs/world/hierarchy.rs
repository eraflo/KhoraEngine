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

//! Parent/child links: reparenting, subtree despawn, ancestry checks.

use khora_core::ecs::entity::EntityId;

use super::World;

impl World {
    /// Re-parents `child`, or detaches it when `new_parent` is `None`.
    ///
    /// The hierarchy is stored twice — [`Parent`] on the child and [`Children`]
    /// on the parent — because both directions are walked every frame. Two
    /// copies of one fact is two chances to disagree, so the only correct place
    /// to write either of them is here, where both move together.
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
        use crate::ecs::{Children, Parent};

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
}

impl World {
    /// Despawns `root` and every entity beneath it, returning how many went.
    ///
    /// Destroying a parent alone would leave its children holding a [`Parent`]
    /// that names a recycled index — the ABA problem the generation counter
    /// exists to catch, reintroduced one level down. A guard holding a weapon as
    /// a child should not leave the weapon floating where it died.
    ///
    /// [`Parent`]: crate::ecs::Parent
    pub fn despawn_subtree(&mut self, root: EntityId) -> usize {
        use crate::ecs::Children;

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
}

impl World {
    /// Whether `candidate` sits under `ancestor` in the hierarchy.
    pub fn is_descendant_of(&self, candidate: EntityId, ancestor: EntityId) -> bool {
        use crate::ecs::Parent;

        let mut current = candidate;
        // Bounded: this is the guard against a malformed hierarchy, so it cannot
        // itself assume the hierarchy is well formed.
        for _ in 0..1024 {
            let Some(parent) = self.get::<Parent>(current) else {
                return false;
            };
            if parent.0 == ancestor {
                return true;
            }
            current = parent.0;
        }
        log::warn!("is_descendant_of: hierarchy traversal exceeded depth bound");
        false
    }
}
