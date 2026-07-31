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

//! Render-interpolation bookkeeping — the per-entity "previous pose" store.
//!
//! The fixed-timestep simulation advances in whole steps while rendering
//! happens once per (variable-rate) frame. To stay smooth between steps, the
//! render path blends each simulated entity's transform *as it was before the
//! latest step* with its current [`GlobalTransform`] by the
//! [`Time::interpolation_alpha`](crate::time::Time::interpolation_alpha).
//!
//! Those "previous" poses live HERE, not in the ECS, on purpose: interpolation
//! is a render-only representation ("adapt the HOW, never the WHAT"). Keeping it
//! out of the component space means it never shows up in the editor inspector
//! and is never written into scene files — it carries no game meaning. The store
//! lives in [`Runtime::resources`](crate::Runtime) behind an `Arc<RwLock<…>>`,
//! mirroring the `CollisionPairs` broadphase scratch: engine-owned per-entity
//! data, keyed by [`EntityId`], that no gameplay code authors.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::ecs::entity::EntityId;
use crate::math::affine_transform::AffineTransform;

/// Per-entity snapshots of the world-space transform *before* the latest
/// simulation step, consumed by the render path for motion interpolation.
///
/// Engine-internal: populated by the data layer's capture pass for simulated
/// bodies and read by the render projection. Not an ECS component (see the
/// module docs) — it is neither inspectable nor serialized.
#[derive(Debug, Default)]
pub struct TransformInterpolation {
    previous: HashMap<EntityId, AffineTransform>,
}

impl TransformInterpolation {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The pre-step world-space transform recorded for `entity`, if the entity
    /// is currently simulated. `None` means "render at the current transform"
    /// (static entity, or first frame before any snapshot).
    pub fn previous(&self, entity: EntityId) -> Option<AffineTransform> {
        self.previous.get(&entity).copied()
    }

    /// Records `entity`'s current world-space transform as the pose to
    /// interpolate *from* on the next render.
    pub fn record(&mut self, entity: EntityId, transform: AffineTransform) {
        self.previous.insert(entity, transform);
    }

    /// Drops snapshots for entities absent from `live` — despawned bodies, or
    /// entities the simulation no longer moves. Bounds memory and prevents a
    /// recycled [`EntityId`] from reading a stale generation's pose.
    pub fn retain_live(&mut self, live: &HashSet<EntityId>) {
        self.previous.retain(|id, _| live.contains(id));
    }

    /// Number of entities with a recorded previous pose.
    pub fn len(&self) -> usize {
        self.previous.len()
    }

    /// Whether no previous pose is recorded.
    pub fn is_empty(&self) -> bool {
        self.previous.is_empty()
    }
}

/// Shared, interior-mutable handle to the [`TransformInterpolation`] store as it
/// lives in [`Runtime::resources`](crate::Runtime): the data layer's capture
/// pass takes the write lock once per frame; the render projection takes a read
/// lock while it projects.
pub type SharedTransformInterpolation = Arc<RwLock<TransformInterpolation>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;

    fn entity(index: u32) -> EntityId {
        EntityId {
            index,
            generation: 0,
        }
    }

    fn at(x: f32) -> AffineTransform {
        AffineTransform(crate::math::Mat4::from_translation(Vec3::new(x, 0.0, 0.0)))
    }

    #[test]
    fn record_then_read_round_trips() {
        let mut store = TransformInterpolation::new();
        assert!(store.previous(entity(0)).is_none());
        store.record(entity(0), at(3.0));
        assert_eq!(
            store.previous(entity(0)).map(|t| t.translation()),
            Some(Vec3::new(3.0, 0.0, 0.0))
        );
    }

    #[test]
    fn record_overwrites_previous_value() {
        let mut store = TransformInterpolation::new();
        store.record(entity(1), at(1.0));
        store.record(entity(1), at(2.0));
        assert_eq!(
            store.previous(entity(1)).map(|t| t.translation()),
            Some(Vec3::new(2.0, 0.0, 0.0))
        );
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn retain_live_drops_absent_entities() {
        let mut store = TransformInterpolation::new();
        store.record(entity(0), at(0.0));
        store.record(entity(1), at(1.0));
        store.record(entity(2), at(2.0));

        let live: HashSet<EntityId> = [entity(0), entity(2)].into_iter().collect();
        store.retain_live(&live);

        assert!(store.previous(entity(0)).is_some());
        assert!(store.previous(entity(1)).is_none(), "despawned id pruned");
        assert!(store.previous(entity(2)).is_some());
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn empty_live_set_clears_the_store() {
        let mut store = TransformInterpolation::new();
        store.record(entity(0), at(0.0));
        store.retain_live(&HashSet::new());
        assert!(store.is_empty());
    }
}
