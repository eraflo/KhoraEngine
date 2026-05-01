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

//! Shared, engine-wide GPU mesh cache.
//!
//! [`GpuCache`] is a named wrapper around the shared `Assets<GpuMesh>` store.
//! The named wrapper is intentional: `ServiceRegistry` keys by `TypeId`, so
//! registering a plain `Arc<RwLock<Assets<GpuMesh>>>` would be ambiguous with
//! any other such arc a plugin might register.
//!
//! `GpuCache` is created once in `engine.rs` bootstrap and registered into
//! `ServiceRegistry`. Every agent that needs GPU mesh handles reads it from
//! there — agents must never maintain an independent `Assets<GpuMesh>`.

use crate::assets::Assets;
use khora_core::renderer::api::scene::GpuMesh;
use std::sync::{Arc, RwLock};

/// Shared, engine-wide GPU mesh store.
///
/// Registered into `ServiceRegistry` during engine bootstrap.
/// Retrieved via `context.services.get::<GpuCache>()` in agent `execute()`.
#[derive(Clone)]
pub struct GpuCache(pub Arc<RwLock<Assets<GpuMesh>>>);

impl GpuCache {
    /// Creates a new, empty GPU cache.
    pub fn new() -> Self {
        GpuCache(Arc::new(RwLock::new(Assets::new())))
    }

    /// Returns a reference to the shared inner asset store.
    ///
    /// Clone the returned `Arc` when you need to hold a reference across frames.
    pub fn inner(&self) -> &Arc<RwLock<Assets<GpuMesh>>> {
        &self.0
    }
}

impl Default for GpuCache {
    fn default() -> Self {
        Self::new()
    }
}
