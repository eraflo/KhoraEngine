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

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard};

use khora_core::asset::AssetUUID;
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::Resident;

use crate::residency::ImageOnGpu;

/// Resource holding the UI image atlas (GPU texture) and an
/// `AssetUUID → AtlasRect` cache mapping each uploaded UI image to its
/// position inside the atlas.
///
/// Registered in [`khora_core::Resources`] at engine init. The `image_on_gpu`
/// residency allocates the GPU atlas lazily via [`UiImageAtlas::ensure_atlas`]
/// and places every UI image in it before the agents run; the UI agent borrows
/// the atlas for rendering through [`UiImageAtlas::lock_atlas`] and reads the
/// rects with [`UiImageAtlas::get_rect`].
pub struct UiImageAtlas {
    /// GPU texture atlas, allocated lazily on first call to
    /// `ensure_atlas` once a graphics device is available.
    atlas: Mutex<Option<TextureAtlas>>,
    /// Persistent `AssetUUID → AtlasRect` cache: the `image_on_gpu`
    /// residency's store, which places an image again when its copy changes.
    residents: Resident<ImageOnGpu>,
    /// The images the atlas refused, so a refusal is logged once.
    refused: Mutex<HashSet<AssetUUID>>,
}

impl UiImageAtlas {
    /// Creates an empty resource. The GPU atlas itself is allocated
    /// lazily by [`ensure_atlas`](Self::ensure_atlas) once a device is
    /// available.
    #[must_use]
    pub fn new() -> Self {
        Self {
            atlas: Mutex::new(None),
            residents: Resident::new(),
            refused: Mutex::new(HashSet::new()),
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
        self.residents.get(id)
    }

    /// Stores `rect` under `id`, with no decoded copy behind it: the
    /// `image_on_gpu` residency places the image again from its source.
    pub fn insert_rect(&self, id: AssetUUID, rect: AtlasRect) {
        self.residents.insert(id, rect);
    }

    /// The `image_on_gpu` residency's store of rects.
    pub fn residents(&self) -> &Resident<ImageOnGpu> {
        &self.residents
    }

    /// Records that the atlas refused `id`; `true` the first time.
    pub(crate) fn note_refusal(&self, id: AssetUUID) -> bool {
        self.refused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id)
    }

    /// Forgets a refusal of `id`, once the image is placed.
    pub(crate) fn forget_refusal(&self, id: &AssetUUID) {
        self.refused
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
    }

    /// Reports whether the GPU atlas has been initialized.
    pub fn has_atlas(&self) -> bool {
        self.atlas.lock().ok().is_some_and(|g| g.is_some())
    }

    /// Returns the number of cache entries.
    pub fn cache_len(&self) -> usize {
        self.residents.len()
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
