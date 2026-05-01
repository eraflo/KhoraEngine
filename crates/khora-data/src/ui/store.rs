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

//! Shared, engine-wide container for the per-frame [`UiScene`].

use std::sync::{Arc, RwLock};

use super::UiScene;

/// Shared, engine-wide container for the per-frame [`UiScene`].
#[derive(Clone, Default)]
pub struct UiSceneStore(Arc<RwLock<UiScene>>);

impl UiSceneStore {
    /// Creates a new, empty store.
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(UiScene::new())))
    }

    /// Returns the shared `Arc<RwLock<UiScene>>`.
    pub fn shared(&self) -> &Arc<RwLock<UiScene>> {
        &self.0
    }
}
