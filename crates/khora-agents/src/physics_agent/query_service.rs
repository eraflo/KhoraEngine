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

//! On-demand physics query service — a direct service, not an Agent.
//!
//! [`PhysicsQueryService`] wraps the shared `PhysicsProvider` and exposes
//! stateless queries (raycasting, debug geometry) that don't require GORNA
//! negotiation.  Registered in `ServiceRegistry` during bootstrap; retrieved
//! by any code that needs queries via `context.services.get::<PhysicsQueryService>()`.

use khora_core::{
    math::Vec3,
    physics::{PhysicsProvider, Ray, RaycastHit},
};
use std::sync::{Arc, Mutex};

/// On-demand physics query service.
///
/// Wraps the shared [`PhysicsProvider`] for stateless queries.
/// Registered into `ServiceRegistry` during engine bootstrap.
///
/// This is a **Service**, not an Agent — it has no GORNA strategy, no
/// `execute()` method, and runs on the caller's thread on demand.
#[derive(Clone)]
pub struct PhysicsQueryService {
    provider: Arc<Mutex<Box<dyn PhysicsProvider>>>,
}

impl PhysicsQueryService {
    /// Creates a new `PhysicsQueryService` backed by the given shared provider.
    pub fn new(provider: Arc<Mutex<Box<dyn PhysicsProvider>>>) -> Self {
        Self { provider }
    }

    /// Casts a ray and returns the first hit, if any.
    ///
    /// # Arguments
    /// * `ray`    – Ray origin + unit direction.
    /// * `max_toi` – Maximum time-of-impact (distance along the ray to test).
    /// * `solid`  – If `true`, the ray starts inside a solid collider and
    ///              returns that collider as the hit. If `false`, the ray
    ///              must exit the solid to register a hit.
    pub fn cast_ray(&self, ray: &Ray, max_toi: f32, solid: bool) -> Option<RaycastHit> {
        self.provider
            .lock()
            .ok()
            .and_then(|g| g.cast_ray(ray, max_toi, solid))
    }

    /// Returns debug line-segment geometry from the physics world.
    ///
    /// Returns a tuple of `(vertices, edges)` where each edge is a pair of
    /// vertex indices.  Useful for rendering physics collider outlines.
    pub fn debug_render_data(&self) -> (Vec<Vec3>, Vec<[u32; 2]>) {
        self.provider
            .lock()
            .ok()
            .map(|g| g.get_debug_render_data())
            .unwrap_or_default()
    }
}
