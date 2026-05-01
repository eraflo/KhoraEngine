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

//! Shared, engine-wide container for the per-frame [`RenderWorld`].
//!
//! `RenderWorldStore` is **data**, not a service: it owns no logic, just an
//! `Arc<RwLock<RenderWorld>>` that lets multiple agents read (and the engine
//! write) the same scene snapshot.  The engine populates it via
//! [`extract_scene`](super::extract_scene) once per frame; agents look it up
//! from the `ServiceRegistry` and acquire a read or write guard as needed.

use std::sync::{Arc, RwLock};

use super::RenderWorld;

/// Shared, engine-wide container for the per-frame [`RenderWorld`].
#[derive(Clone, Default)]
pub struct RenderWorldStore(Arc<RwLock<RenderWorld>>);

impl RenderWorldStore {
    /// Creates a new, empty store.
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(RenderWorld::new())))
    }

    /// Returns the shared `Arc<RwLock<RenderWorld>>`.
    pub fn shared(&self) -> &Arc<RwLock<RenderWorld>> {
        &self.0
    }
}
