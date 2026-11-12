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

//! What a lit lane binds at group 3 when the frame has no shadows or no
//! image-based lighting: objects of its own that light nothing.
//!
//! A frame may come without the shadows family's atlases (it failed, or the
//! engine runs it at no level) and before the first IBL bake. The pipeline
//! still declares those bindings, so the lane binds its own: a 1×1 depth atlas
//! of each kind with a comparison sampler — every light it shades has no
//! shadow entry, so the shader never samples them — and two black cubes, a
//! zero BRDF lookup and a filtering sampler, so the environment adds no light.
//! Textures are zeroed when the device creates them.

use std::borrow::Cow;

use khora_core::math::Extent3D;
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::{
    AddressMode, FilterMode, ImageAspect, MipmapFilterMode, SampleCount, SamplerDescriptor,
    SamplerId, TextureDescriptor, TextureDimension, TextureFormat, TextureId, TextureUsage,
    TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::shadow::ShadowGpuBindings;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::GraphicsDevice;

use crate::shadows::algo::atlas_2d::Atlas2D;
use crate::shadows::algo::atlas_cube::AtlasCube;
use crate::shadows::algo::state::shadow_sampler_descriptor;

/// The format of the neutral environment cubes.
const CUBE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The format of the neutral BRDF lookup, the bake's.
const LUT_FORMAT: TextureFormat = TextureFormat::Rg16Float;

/// A lit lane's own shadow and environment bindings.
pub(crate) struct NeutralBindings {
    /// What group 3 binds in place of the shadows family's atlases.
    pub shadows: ShadowGpuBindings,
    /// What group 3 binds in place of the bake's environment.
    pub ibl: IblGpuBindings,
    atlas_2d: Atlas2D,
    atlas_cube: AtlasCube,
    shadow_sampler: SamplerId,
    textures: Vec<TextureId>,
    views: Vec<TextureViewId>,
    ibl_sampler: SamplerId,
}

impl NeutralBindings {
    /// Builds every neutral binding; on a failure, gives back what was
    /// already built.
    pub fn build(device: &dyn GraphicsDevice, label: &str) -> Result<Self, RenderError> {
        let mut built = Built::default();
        match Self::build_into(device, label, &mut built) {
            Ok(neutral) => Ok(neutral),
            Err(error) => {
                built.destroy(device);
                Err(error)
            }
        }
    }

    fn build_into(
        device: &dyn GraphicsDevice,
        label: &str,
        built: &mut Built,
    ) -> Result<Self, RenderError> {
        let atlas_2d = Atlas2D::create(device, 1, 1, &format!("{label} Neutral Shadow 2D"))?;
        built.atlas_2d = Some((atlas_2d.texture, atlas_2d.view));
        let atlas_cube = AtlasCube::create(device, 1, 1, &format!("{label} Neutral Shadow Cube"))?;
        built.cube_views = atlas_cube.face_views.to_vec();
        built.cube_views.push(atlas_cube.view);
        built.cube_texture = Some(atlas_cube.texture);
        let shadow_sampler = device
            .create_sampler(&shadow_sampler_descriptor())
            .map_err(RenderError::ResourceError)?;
        built.samplers.push(shadow_sampler);

        let irradiance = texture(device, built, label, "Irradiance", CUBE_FORMAT, 6, true)?;
        let prefiltered = texture(device, built, label, "Prefiltered", CUBE_FORMAT, 6, true)?;
        let brdf_lut = texture(device, built, label, "BRDF LUT", LUT_FORMAT, 1, false)?;
        let ibl_sampler = device
            .create_sampler(&SamplerDescriptor {
                label: Some(Cow::Owned(format!("{label} Neutral IBL Sampler"))),
                address_mode_u: AddressMode::ClampToEdge,
                address_mode_v: AddressMode::ClampToEdge,
                address_mode_w: AddressMode::ClampToEdge,
                mag_filter: FilterMode::Linear,
                min_filter: FilterMode::Linear,
                mipmap_filter: MipmapFilterMode::Linear,
                lod_min_clamp: 0.0,
                lod_max_clamp: 0.0,
                compare: None,
                anisotropy_clamp: 1,
                border_color: None,
            })
            .map_err(RenderError::ResourceError)?;

        Ok(Self {
            shadows: ShadowGpuBindings {
                atlas_2d: atlas_2d.view,
                atlas_cube: atlas_cube.view,
                sampler: shadow_sampler,
            },
            ibl: IblGpuBindings {
                env_cube: irradiance,
                irradiance_cube: irradiance,
                prefiltered_cube: prefiltered,
                brdf_lut,
                sampler: ibl_sampler,
            },
            atlas_2d,
            atlas_cube,
            shadow_sampler,
            textures: std::mem::take(&mut built.textures),
            views: std::mem::take(&mut built.views),
            ibl_sampler,
        })
    }

    /// Gives every neutral binding back to the device.
    pub fn destroy(self, device: &dyn GraphicsDevice) {
        self.atlas_2d.destroy(device);
        self.atlas_cube.destroy(device);
        warn_on(device.destroy_sampler(self.shadow_sampler));
        warn_on(device.destroy_sampler(self.ibl_sampler));
        for view in self.views {
            warn_on(device.destroy_texture_view(view));
        }
        for texture in self.textures {
            warn_on(device.destroy_texture(texture));
        }
    }
}

/// What a build made so far, given back when a later step fails.
#[derive(Default)]
struct Built {
    atlas_2d: Option<(TextureId, TextureViewId)>,
    cube_texture: Option<TextureId>,
    cube_views: Vec<TextureViewId>,
    samplers: Vec<SamplerId>,
    textures: Vec<TextureId>,
    views: Vec<TextureViewId>,
}

impl Built {
    fn destroy(self, device: &dyn GraphicsDevice) {
        if let Some((texture, view)) = self.atlas_2d {
            warn_on(device.destroy_texture_view(view));
            warn_on(device.destroy_texture(texture));
        }
        for view in self.cube_views.into_iter().chain(self.views) {
            warn_on(device.destroy_texture_view(view));
        }
        for texture in self.cube_texture.into_iter().chain(self.textures) {
            warn_on(device.destroy_texture(texture));
        }
        for sampler in self.samplers {
            warn_on(device.destroy_sampler(sampler));
        }
    }
}

/// A 1×1 texture of `layers` layers and its sampling view — a cube view
/// when `cube`, else a 2D one — recorded in `built`.
fn texture(
    device: &dyn GraphicsDevice,
    built: &mut Built,
    label: &str,
    what: &str,
    format: TextureFormat,
    layers: u32,
    cube: bool,
) -> Result<TextureViewId, RenderError> {
    let texture = device
        .create_texture(&TextureDescriptor {
            label: Some(Cow::Owned(format!("{label} Neutral {what}"))),
            size: Extent3D {
                width: 1,
                height: 1,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsage::TEXTURE_BINDING,
            view_formats: Cow::Borrowed(&[]),
        })
        .map_err(RenderError::ResourceError)?;
    built.textures.push(texture);
    let view = device
        .create_texture_view(
            texture,
            &TextureViewDescriptor {
                label: Some(Cow::Owned(format!("{label} Neutral {what} View"))),
                format: Some(format),
                dimension: Some(if cube {
                    TextureViewDimension::Cube
                } else {
                    TextureViewDimension::D2
                }),
                aspect: ImageAspect::All,
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: 0,
                array_layer_count: Some(layers),
            },
        )
        .map_err(RenderError::ResourceError)?;
    built.views.push(view);
    Ok(view)
}

fn warn_on(result: Result<(), ResourceError>) {
    if let Err(error) = result {
        log::warn!("a neutral binding could not be destroyed: {error:?}");
    }
}
