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

//! [`UiImageAtlas`]: the UI images' GPU atlas and the rect of each image in it.
//!
//! A runtime resource the `image_on_gpu` residency fills before the agents run;
//! a full atlas is built again from the images in use.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::renderer::api::resource::{CpuTexture, TextureFormat};
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::Resident;
use khora_core::sync::LockExt;

use crate::residency::{AtlasTarget, ImageOnGpu};

/// The side of the atlas [`UiImageAtlas::new`] makes, in pixels.
const DEFAULT_SIZE: u32 = 2048;

/// The label of the atlas texture.
const LABEL: &str = "ui_image_atlas";

/// The UI images' GPU atlas, and the rect each image was placed at.
///
/// A runtime resource. The `image_on_gpu` residency makes the atlas and fills
/// it; the UI lane reads it through [`lock_atlas`](Self::lock_atlas) and
/// [`get_rect`](Self::get_rect). The atlas packs rects one after the other and
/// frees none: when an image does not fit, it is built again, once a run, from
/// the images in use, and the old texture and its view are destroyed.
///
/// # Locks
///
/// The atlas's `Mutex` sits above the device's leaf lock: it is held across the
/// device calls that make, fill or destroy its own texture, never across a call
/// that calls back. The store of rects, the refusals and the images left out
/// are leaves below it.
pub struct UiImageAtlas {
    /// The side of the atlas texture, in pixels.
    size: u32,
    /// GPU texture atlas, made on the first image placed once a graphics
    /// device is available.
    atlas: Mutex<Option<TextureAtlas>>,
    /// Persistent `AssetUUID → AtlasRect` cache: the `image_on_gpu`
    /// residency's store, which places an image again when its copy changes.
    residents: Resident<ImageOnGpu>,
    /// The images the atlas refused, so a refusal is logged once.
    refused: Mutex<HashSet<AssetUUID>>,
    /// The images a rebuilt atlas still had no room for, and the copies in use
    /// then.
    ///
    /// Tried again once one of those copies goes out of use: only an image
    /// going away, or replaced, makes room.
    left_out: Mutex<Option<LeftOut>>,
}

impl UiImageAtlas {
    /// Creates an empty resource whose atlas, once made, is 2048 pixels
    /// square.
    #[must_use]
    pub fn new() -> Self {
        Self::with_size(DEFAULT_SIZE)
    }

    /// Creates an empty resource whose atlas, once made, is `size` pixels
    /// square.
    #[must_use]
    pub fn with_size(size: u32) -> Self {
        Self {
            size,
            atlas: Mutex::new(None),
            residents: Resident::new(),
            refused: Mutex::new(HashSet::new()),
            left_out: Mutex::new(None),
        }
    }

    /// Makes the GPU atlas if it is not made yet; idempotent.
    ///
    /// Returns whether the atlas exists after the call.
    pub fn ensure_atlas(&self, device: &dyn GraphicsDevice) -> bool {
        let Some(mut guard) = self.lock_atlas() else {
            return false;
        };
        self.made(&mut guard, device).is_some()
    }

    /// Locks the GPU atlas, for the UI lane's read of its view.
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

    /// Reports whether the GPU atlas has been initialized.
    pub fn has_atlas(&self) -> bool {
        self.atlas.lock().ok().is_some_and(|g| g.is_some())
    }

    /// Returns the number of cache entries.
    pub fn cache_len(&self) -> usize {
        self.residents.len()
    }

    /// Returns the copy the atlas draws for each image in use, making the atlas
    /// if it is not yet; `None` when it cannot be made.
    ///
    /// `in_use` pairs each image a live `UiImage` names with its current copy,
    /// `None` while it is not decoded (a reload in flight). An image draws its
    /// current copy when that copy fits an empty atlas; otherwise its last good
    /// copy: the one `left_out` holds for it, else the one its rect was made
    /// from. An image with neither draws nothing.
    pub(crate) fn drawable(
        &self,
        device: &dyn GraphicsDevice,
        in_use: &[(AssetUUID, Option<AssetHandle<CpuTexture>>)],
        left_out: &HashMap<AssetUUID, AssetHandle<CpuTexture>>,
    ) -> Option<Drawables> {
        let mut guard = self.lock_atlas()?;
        let atlas = self.made(&mut guard, device)?;
        let mut drawables = Drawables::default();
        for (id, current) in in_use {
            if let Some(current) = current {
                if fits_when_empty(atlas, current) {
                    drawables.copies.push(Drawable {
                        id: *id,
                        copy: current.clone(),
                        current: true,
                    });
                    continue;
                }
                drawables.too_large.push((*id, current.clone()));
            }
            let last_good = left_out
                .get(id)
                .cloned()
                .or_else(|| self.residents.source(id));
            if let Some(copy) = last_good {
                drawables.copies.push(Drawable {
                    id: *id,
                    copy,
                    current: false,
                });
            }
        }
        Some(drawables)
    }

    /// Places `copy` as the image `id` names, making the atlas if it is not
    /// yet; a copy already placed uploads nothing.
    ///
    /// `copy` is one [`drawable`](Self::drawable) chose: it fits an empty atlas.
    pub(crate) fn place(
        &self,
        device: &dyn GraphicsDevice,
        id: AssetUUID,
        copy: &AssetHandle<CpuTexture>,
    ) -> Placement {
        let Some(mut guard) = self.lock_atlas() else {
            return Placement::NoAtlas;
        };
        let Some(atlas) = self.made(&mut guard, device) else {
            return Placement::NoAtlas;
        };
        let mut target = AtlasTarget { device, atlas };
        match self.residents.ensure(id, copy, &mut target) {
            Ok(_) => Placement::Placed,
            Err(_) => Placement::Full,
        }
    }

    /// Builds the atlas again with `images` alone, destroying the old texture
    /// and its view.
    ///
    /// Every rect of the old atlas goes; `images` are the copies
    /// [`drawable`](Self::drawable) chose, so none of them is lost with it.
    /// Returns the images the fresh atlas has no room for, each with its copy;
    /// `None` when the device cannot make the fresh atlas, which leaves the old
    /// one as it was.
    pub(crate) fn rebuild(
        &self,
        device: &dyn GraphicsDevice,
        images: &[Drawable],
    ) -> Option<Vec<(AssetUUID, AssetHandle<CpuTexture>)>> {
        let mut guard = self.lock_atlas()?;
        let mut fresh = match TextureAtlas::new(device, self.size, TextureFormat::Rgba8Unorm, LABEL)
        {
            Ok(fresh) => fresh,
            Err(error) => {
                log::error!("UiImageAtlas: failed to rebuild the GPU atlas: {error:?}");
                return None;
            }
        };
        match guard.take() {
            Some(mut old) => {
                self.residents.evict_all(&mut AtlasTarget {
                    device,
                    atlas: &mut old,
                });
                destroy(device, &old);
            }
            None => self.residents.evict_all(&mut AtlasTarget {
                device,
                atlas: &mut fresh,
            }),
        }
        let atlas = guard.insert(fresh);
        let mut target = AtlasTarget { device, atlas };
        let mut left_out = Vec::new();
        for image in images {
            // No room left for this one; a smaller one may still fit.
            if self
                .residents
                .ensure(image.id, &image.copy, &mut target)
                .is_err()
            {
                left_out.push((image.id, image.copy.clone()));
            }
        }
        Some(left_out)
    }

    /// Returns the images a rebuilt atlas left out, and the copies in use then.
    pub(crate) fn left_out(&self) -> Option<LeftOut> {
        self.left_out.lock_recovered().clone()
    }

    /// Remembers the images a rebuilt atlas left out, with the copies in use
    /// now; none left out forgets them.
    pub(crate) fn set_left_out(&self, left_out: LeftOut) {
        *self.left_out.lock_recovered() = (!left_out.images.is_empty()).then_some(left_out);
    }

    /// Records that the atlas refused `id`; `true` the first time.
    pub(crate) fn note_refusal(&self, id: AssetUUID) -> bool {
        self.refused.lock_recovered().insert(id)
    }

    /// Forgets a refusal of `id`, once the image is placed.
    pub(crate) fn forget_refusal(&self, id: &AssetUUID) {
        self.refused.lock_recovered().remove(id);
    }

    /// Returns the atlas in `slot`, made on first need.
    fn made<'a>(
        &self,
        slot: &'a mut Option<TextureAtlas>,
        device: &dyn GraphicsDevice,
    ) -> Option<&'a mut TextureAtlas> {
        if slot.is_none() {
            match TextureAtlas::new(device, self.size, TextureFormat::Rgba8Unorm, LABEL) {
                Ok(atlas) => *slot = Some(atlas),
                Err(e) => {
                    log::error!("UiImageAtlas: failed to allocate GPU atlas: {:?}", e);
                    return None;
                }
            }
        }
        slot.as_mut()
    }
}

impl Default for UiImageAtlas {
    fn default() -> Self {
        Self::new()
    }
}

/// The images a rebuilt atlas left out, and the copies drawn then.
#[derive(Clone, Default)]
pub(crate) struct LeftOut {
    /// The images left out, each with the copy that found no room: its last
    /// good copy until it is placed.
    pub(crate) images: HashMap<AssetUUID, AssetHandle<CpuTexture>>,
    /// Every image in use when they were left out, with the copy it drew.
    pub(crate) in_use: HashMap<AssetUUID, AssetHandle<CpuTexture>>,
}

/// The copies the atlas draws for the images in use, and the current copies
/// too large for it.
#[derive(Default)]
pub(crate) struct Drawables {
    /// The copy each image in use draws, in the order the world names them.
    pub(crate) copies: Vec<Drawable>,
    /// The current copies larger than an empty atlas: no rebuild makes room.
    pub(crate) too_large: Vec<(AssetUUID, AssetHandle<CpuTexture>)>,
}

/// The copy one image in use draws.
#[derive(Clone)]
pub(crate) struct Drawable {
    /// The image.
    pub(crate) id: AssetUUID,
    /// The copy drawn: its current one, or its last good one.
    pub(crate) copy: AssetHandle<CpuTexture>,
    /// Whether `copy` is the image's current copy.
    pub(crate) current: bool,
}

/// What placing an image came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// It is in the atlas.
    Placed,
    /// The atlas has no room left: a rebuild may make some.
    Full,
    /// The atlas cannot be made, or its lock is poisoned.
    NoAtlas,
}

/// Returns whether `copy` fits `atlas` once empty.
fn fits_when_empty(atlas: &TextureAtlas, copy: &CpuTexture) -> bool {
    atlas.could_hold(copy.size.width, copy.size.height)
}

/// Destroys the texture of `atlas` and its view, logging what the device refuses.
fn destroy(device: &dyn GraphicsDevice, atlas: &TextureAtlas) {
    if let Err(error) = device.destroy_texture_view(atlas.view()) {
        log::warn!("UiImageAtlas: destroying the old view failed: {error}");
    }
    if let Err(error) = device.destroy_texture(atlas.texture()) {
        log::warn!("UiImageAtlas: destroying the old texture failed: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::tests::device::RecordingDevice;

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

    /// **The default atlas is 2048 px square.**
    #[test]
    fn the_default_atlas_is_2048_pixels_square() {
        let device = RecordingDevice::default();
        assert!(UiImageAtlas::new().ensure_atlas(&device));
        let size = device.textures()[0].size;
        assert_eq!((size.width, size.height), (2048, 2048));
    }

    /// **An atlas made with a size is a square of that size**, made once.
    #[test]
    fn an_atlas_with_a_size_is_a_square_of_that_size() {
        let device = RecordingDevice::default();
        let atlas = UiImageAtlas::with_size(64);
        assert!(atlas.ensure_atlas(&device));
        assert!(atlas.ensure_atlas(&device), "idempotent");
        let textures = device.textures();
        assert_eq!(textures.len(), 1, "made once");
        assert_eq!((textures[0].size.width, textures[0].size.height), (64, 64));
    }
}
