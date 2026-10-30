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

//! `ImageOnGpu` — UI images placed in the GPU atlas.
//!
//! Every image a `UiImage` names is uploaded into the
//! [`UiImageAtlas`](crate::ui::UiImageAtlas) before the agents run, and placed
//! again when its file changes: the atlas's [`Resident`] store remembers which
//! decoded copy each rect was made from, and the asset reload replaces that
//! copy. The UI agent only reads the rects.
//!
//! [`Resident`]: khora_core::residency::Resident

use crate::ecs::SystemContext;
use std::collections::HashSet;
use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::renderer::GraphicsDevice;
use khora_core::residency::{DeviceClass, Residency, ResidencyError};
use khora_core::Runtime;

use crate::ecs::World;
use crate::ui::components::UiImage;
use crate::ui::UiImageAtlas;
use crate::AssetStore;

/// A UI image, as a rect of the GPU atlas.
pub struct ImageOnGpu;

/// What an image is placed on: the atlas, and the device it lives on.
pub struct AtlasTarget<'a> {
    /// The device the atlas texture belongs to.
    pub device: &'a dyn GraphicsDevice,
    /// The atlas the image is placed in.
    pub atlas: &'a mut TextureAtlas,
}

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

    /// The atlas packs rects one after the other and frees none: the space of
    /// a replaced image comes back only when the atlas is made again.
    fn release(_resident: AtlasRect, _target: &mut AtlasTarget<'_>) {}
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
    let mut seen = HashSet::new();
    let wanted: Vec<AssetUUID> = world
        .query::<&UiImage>()
        .map(|image| image.texture)
        .filter(|texture| seen.insert(*texture))
        .collect();
    if wanted.is_empty() || !atlas.ensure_atlas(device.as_ref()) {
        return;
    }
    let textures = store.store::<CpuTexture>();
    let textures = textures.read().unwrap_or_else(|e| e.into_inner());
    let Some(mut guard) = atlas.lock_atlas() else {
        return;
    };
    let Some(texture_atlas) = guard.as_mut() else {
        return;
    };
    let mut target = AtlasTarget {
        device: device.as_ref(),
        atlas: texture_atlas,
    };
    for uuid in wanted {
        // Not decoded yet: the resolver loads it, a later frame places it.
        let Some(source) = textures.get(&uuid) else {
            continue;
        };
        match atlas.residents().ensure(uuid, source, &mut target) {
            Ok(_) => atlas.forget_refusal(&uuid),
            Err(refusal) => {
                if atlas.note_refusal(uuid) {
                    log::warn!("{refusal}");
                }
            }
        }
    }
}

crate::register_residency!(name = ImageOnGpu::NAME, run = image_on_gpu_system);
