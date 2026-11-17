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

//! [`UiImageAtlas`]: the UI images' GPU atlas and the rect of each image in it,
//! a runtime resource the `image_on_gpu` residency fills before the agents run.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard};

use khora_core::asset::AssetUUID;
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::Resident;
use khora_core::sync::LockExt;

use crate::residency::ImageOnGpu;

/// The UI images' GPU atlas, and the rect each image was placed at.
///
/// A runtime resource. The `image_on_gpu` residency makes the atlas with
/// [`ensure_atlas`](Self::ensure_atlas) and fills it; the UI lane reads it
/// through [`lock_atlas`](Self::lock_atlas) and [`get_rect`](Self::get_rect).
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
    /// Creates an empty resource; [`ensure_atlas`](Self::ensure_atlas) makes
    /// the GPU atlas.
    #[must_use]
    pub fn new() -> Self {
        Self {
            atlas: Mutex::new(None),
            residents: Resident::new(),
            refused: Mutex::new(HashSet::new()),
        }
    }

    /// Makes the GPU atlas if it is not made yet; idempotent.
    ///
    /// Returns whether the atlas exists after the call.
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

    /// Locks the GPU atlas, for the residency's upload or the UI lane's read
    /// of its view.
    ///
    /// `None` when the lock is poisoned. Hold the guard no longer than the
    /// work needs.
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

    /// Returns the `image_on_gpu` residency's store of rects.
    pub fn residents(&self) -> &Resident<ImageOnGpu> {
        &self.residents
    }

    /// Records that the atlas refused `id`; `true` the first time.
    pub(crate) fn note_refusal(&self, id: AssetUUID) -> bool {
        self.refused.lock_recovered().insert(id)
    }

    /// Forgets a refusal of `id`, once the image is placed.
    pub(crate) fn forget_refusal(&self, id: &AssetUUID) {
        self.refused.lock_recovered().remove(id);
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
