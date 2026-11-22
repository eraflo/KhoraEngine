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

//! `ImageOnGpu`: every image a `UiImage` names, placed in the
//! [`UiImageAtlas`] before the agents run, and placed
//! again when its file changes; a full atlas is built again from the images
//! in use.

use crate::ecs::SystemContext;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::{DeviceClass, Residency, ResidencyError};
use khora_core::sync::RwLockExt;
use khora_core::Runtime;

use crate::ecs::World;
use crate::ui::components::UiImage;
use crate::ui::image_atlas::{LeftOut, Placement};
use crate::ui::UiImageAtlas;
use crate::AssetStore;

/// A UI image, as a rect of the GPU atlas.
pub struct ImageOnGpu;

impl Residency for ImageOnGpu {
    const NAME: &'static str = "image_on_gpu";
    const DEVICE: DeviceClass = DeviceClass::Gpu;
    type Source = CpuTexture;
    type Resident = AtlasRect;
    type Target<'a> = AtlasTarget<'a>;

    fn make(
        source: &CpuTexture,
        target: &mut AtlasTarget<'_>,
    ) -> Result<AtlasRect, ResidencyError> {
        let (width, height) = (source.size.width, source.size.height);
        target
            .atlas
            .allocate_and_upload(
                target.device,
                width,
                height,
                &source.pixels,
                source.format.bytes_per_pixel(),
            )
            .ok_or_else(|| ResidencyError {
                residency: Self::NAME,
                reason: format!("a {width}×{height} image does not fit in the UI atlas"),
            })
    }

    /// Gives nothing back: the atlas frees no rect alone.
    ///
    /// A released rect stays unused until the atlas, out of room, is built
    /// again from the images in use.
    fn release(_resident: AtlasRect, _target: &mut AtlasTarget<'_>) {}
}

/// What an image is placed on: the atlas, and the device it lives on.
pub struct AtlasTarget<'a> {
    /// The device the atlas texture belongs to.
    pub device: &'a dyn GraphicsDevice,
    /// The atlas the image is placed in.
    pub atlas: &'a mut TextureAtlas,
}

fn image_on_gpu_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(atlas) = runtime.resources.get::<Arc<UiImageAtlas>>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    let Some(store) = runtime.resources.get::<AssetStore>() else {
        return;
    };
    let device = device.as_ref();
    // In use: named by a live `UiImage`, decoded or not. An image not decoded
    // yet, or reloading, has no current copy.
    let mut seen = HashSet::new();
    let in_use: Vec<(AssetUUID, Option<AssetHandle<CpuTexture>>)> = {
        let textures = store.store::<CpuTexture>();
        let textures = textures.read_recovered();
        world
            .query::<&UiImage>()
            .map(|image| image.texture)
            .filter(|texture| seen.insert(*texture))
            .map(|uuid| (uuid, textures.get(&uuid).cloned()))
            .collect()
    };
    if in_use.is_empty() {
        return;
    }

    let before = atlas.left_out();
    let empty = HashMap::new();
    let Some(drawables) = atlas.drawable(
        device,
        &in_use,
        before.as_ref().map_or(&empty, |before| &before.images),
    ) else {
        return;
    };
    let copies = &drawables.copies;
    for (uuid, copy) in &drawables.too_large {
        if atlas.note_refusal(*uuid) {
            log::warn!(
                "{}: image {uuid}, {}×{}, is larger than the UI atlas: its last good copy, \
                 if any, is drawn",
                ImageOnGpu::NAME,
                copy.size.width,
                copy.size.height
            );
        }
    }

    // The images a rebuilt atlas left out are tried again once a copy drawn
    // then is gone or replaced: only that makes room.
    let mut rebuilt = false;
    let mut left_out: HashMap<AssetUUID, AssetHandle<CpuTexture>> = HashMap::new();
    if let Some(before) = before {
        let now: HashMap<AssetUUID, &AssetHandle<CpuTexture>> = copies
            .iter()
            .map(|drawable| (drawable.id, &drawable.copy))
            .collect();
        if before
            .in_use
            .iter()
            .any(|(uuid, copy)| now.get(uuid) != Some(&copy))
        {
            rebuilt = true;
            let Some(out) = atlas.rebuild(device, copies) else {
                return;
            };
            left_out.extend(out);
        } else {
            left_out = before.images;
        }
    }

    // Each copy placed; a full atlas is built again from every copy drawn,
    // once a run.
    for drawable in copies {
        if left_out.get(&drawable.id) == Some(&drawable.copy) {
            continue;
        }
        left_out.remove(&drawable.id);
        match atlas.place(device, drawable.id, &drawable.copy) {
            Placement::Placed => {}
            Placement::NoAtlas => return,
            Placement::Full if rebuilt => {
                left_out.insert(drawable.id, drawable.copy.clone());
            }
            Placement::Full => {
                rebuilt = true;
                let Some(out) = atlas.rebuild(device, copies) else {
                    return;
                };
                left_out.extend(out);
            }
        }
    }

    // A refusal is logged once, and again only after its current copy was
    // placed since.
    for drawable in copies.iter().filter(|drawable| drawable.current) {
        if !left_out.contains_key(&drawable.id) {
            atlas.forget_refusal(&drawable.id);
        }
    }
    for (uuid, copy) in &left_out {
        if atlas.note_refusal(*uuid) {
            log::warn!(
                "{}: the UI atlas, built again from the images in use, has no room for \
                 image {uuid} ({}×{}); it is placed once an image in use goes away",
                ImageOnGpu::NAME,
                copy.size.width,
                copy.size.height
            );
        }
    }
    // What was left out is remembered with the copies drawn now.
    let in_use = if left_out.is_empty() {
        HashMap::new()
    } else {
        copies
            .iter()
            .map(|drawable| (drawable.id, drawable.copy.clone()))
            .collect()
    };
    atlas.set_left_out(LeftOut {
        images: left_out,
        in_use,
    });
}

crate::register_residency!(name = ImageOnGpu::NAME, run = image_on_gpu_system);
