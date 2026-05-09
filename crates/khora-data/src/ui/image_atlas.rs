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

//! `UiImageAtlas` — engine-level resource holding the GPU texture atlas
//! and the persistent `AssetUUID → AtlasRect` cache used by the UI render
//! pipeline.
//!
//! Previously these two pieces lived as fields on `UiAgent`, which made
//! the agent own GPU state in violation of the "agents stay strategists,
//! no buffered output" rule. They now live in
//! [`khora_core::Resources`] so the agent can be recreated freely
//! without losing the atlas, and the UiAgent code holds no GPU state at
//! all.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, RwLock};

use khora_core::asset::AssetUUID;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas, TextureFormat};
use khora_core::renderer::GraphicsDevice;

/// Resource holding the UI image atlas (GPU texture) and an
/// `AssetUUID → AtlasRect` cache mapping each uploaded UI image to its
/// position inside the atlas.
///
/// Registered in [`khora_core::Resources`] at engine init; the UI agent
/// allocates the GPU atlas lazily via [`UiImageAtlas::ensure_atlas`] and
/// then borrows it for upload + render through [`UiImageAtlas::lock_atlas`].
pub struct UiImageAtlas {
    /// GPU texture atlas, allocated lazily on first call to
    /// `ensure_atlas` once a graphics device is available.
    atlas: Mutex<Option<TextureAtlas>>,
    /// Persistent `AssetUUID → AtlasRect` cache.
    cache: RwLock<HashMap<AssetUUID, AtlasRect>>,
}

impl UiImageAtlas {
    /// Creates an empty resource. The GPU atlas itself is allocated
    /// lazily by [`ensure_atlas`](Self::ensure_atlas) once a device is
    /// available.
    #[must_use]
    pub fn new() -> Self {
        Self {
            atlas: Mutex::new(None),
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Allocates the GPU texture atlas if not yet present. Idempotent —
    /// safe to call every frame, the actual allocation happens once.
    ///
    /// Returns `true` when the atlas is available after the call,
    /// `false` if allocation failed.
    pub fn ensure_atlas(&self, device: &dyn GraphicsDevice) -> bool {
        let Ok(mut guard) = self.atlas.lock() else {
            log::error!("UiImageAtlas: mutex poisoned in ensure_atlas");
            return false;
        };
        if guard.is_some() {
            return true;
        }
        match TextureAtlas::new(device, 2048, TextureFormat::Rgba8Unorm, "ui_image_atlas") {
            Ok(atlas) => {
                *guard = Some(atlas);
                true
            }
            Err(e) => {
                log::error!("UiImageAtlas: failed to allocate GPU atlas: {:?}", e);
                false
            }
        }
    }

    /// Locks the GPU atlas for the duration of a frame's upload + render.
    ///
    /// Returns the locked `MutexGuard<Option<TextureAtlas>>`; callers
    /// dereference twice (`guard.as_mut()`) to reach the optional
    /// `&mut TextureAtlas`. The guard is held for the lifetime of the
    /// caller's local — typically `UiAgent::execute` for one frame.
    pub fn lock_atlas(&self) -> Option<MutexGuard<'_, Option<TextureAtlas>>> {
        match self.atlas.lock() {
            Ok(g) => Some(g),
            Err(e) => {
                log::error!("UiImageAtlas: mutex poisoned: {}", e);
                None
            }
        }
    }

    /// Returns the `AtlasRect` previously stored for `id`, if any.
    pub fn get_rect(&self, id: &AssetUUID) -> Option<AtlasRect> {
        self.cache.read().ok().and_then(|m| m.get(id).copied())
    }

    /// Stores `rect` under `id`.
    pub fn insert_rect(&self, id: AssetUUID, rect: AtlasRect) {
        if let Ok(mut m) = self.cache.write() {
            m.insert(id, rect);
        }
    }

    /// Reports whether the GPU atlas has been initialized.
    pub fn has_atlas(&self) -> bool {
        self.atlas.lock().ok().is_some_and(|g| g.is_some())
    }

    /// Returns the number of cache entries.
    pub fn cache_len(&self) -> usize {
        self.cache.read().map(|m| m.len()).unwrap_or(0)
    }
}

impl Default for UiImageAtlas {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_atlas_has_no_gpu_resource() {
        let atlas = UiImageAtlas::new();
        assert!(!atlas.has_atlas());
        assert_eq!(atlas.cache_len(), 0);
    }

    #[test]
    fn cache_insert_and_get() {
        let atlas = UiImageAtlas::new();
        let id = AssetUUID::new_v5("test");
        let rect = AtlasRect::default();
        atlas.insert_rect(id, rect);
        assert_eq!(atlas.get_rect(&id), Some(rect));
        assert_eq!(atlas.cache_len(), 1);
    }

    #[test]
    fn missing_returns_none() {
        let atlas = UiImageAtlas::new();
        assert!(atlas.get_rect(&AssetUUID::new_v5("absent")).is_none());
    }
}
