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

//! The per-entity "previous pose" store the render path interpolates from.
//!
//! It lives in [`Runtime::resources`](crate::Runtime), not in the ECS: it is
//! never inspected nor saved.
//!
//! See the book: concepts/the-frame.md, "Why interpolation is render-only".

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::ecs::entity::EntityId;
use crate::math::affine_transform::AffineTransform;

/// Per-entity world-space transforms from before the latest simulation step.
///
/// Written by the data layer's capture pass for simulated bodies, read by the
/// render projection.
#[derive(Debug, Default)]
pub struct TransformInterpolation {
    previous: HashMap<EntityId, AffineTransform>,
}

impl TransformInterpolation {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The pre-step world-space transform recorded for `entity`, if it is simulated.
    ///
    /// `None` means render at the current transform: a static entity, or no
    /// snapshot yet.
    pub fn previous(&self, entity: EntityId) -> Option<AffineTransform> {
        self.previous.get(&entity).copied()
    }

    /// Records `entity`'s current world-space transform as the pose to
    /// interpolate *from* on the next render.
    pub fn record(&mut self, entity: EntityId, transform: AffineTransform) {
        self.previous.insert(entity, transform);
    }

    /// Drops snapshots for entities absent from `live`.
    ///
    /// Bounds memory and keeps a recycled [`EntityId`] from reading a stale
    /// generation's pose.
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

/// The shared [`TransformInterpolation`] store, as the runtime's resources hold it.
///
/// The capture pass takes the write lock once per frame; the render projection
/// takes a read lock while it projects.
pub type SharedTransformInterpolation = Arc<RwLock<TransformInterpolation>>;

#[cfg(test)]
mod tests;
