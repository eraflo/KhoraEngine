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

//! The wgpu device's textures, texture views and samplers: allocating
//! textures (with the VRAM they count), writing to them, building views and
//! samplers, removing each.

use khora_core::math::dimension;
use khora_core::renderer::api::pipeline::enums::CompareFunction;
use khora_core::renderer::api::resource::texture::{self as api_tex};
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::ResourceError;

use crate::graphics::wgpu::conversions::IntoWgpu;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use wgpu;

use super::entries::{WgpuSamplerEntry, WgpuTextureEntry, WgpuTextureViewEntry};
use super::WgpuDevice;

impl WgpuDevice {
    /// Allocates a texture, counts its size as VRAM, and registers it under
    /// a new id.
    pub(super) fn allocate_texture(
        &self,
        descriptor: &api_tex::TextureDescriptor,
    ) -> Result<api_tex::TextureId, ResourceError> {
        let wgpu_texture_descriptor = wgpu::TextureDescriptor {
            label: descriptor.label.as_deref(),
            size: descriptor.size.into_wgpu(),
            mip_level_count: descriptor.mip_level_count,
            sample_count: descriptor.sample_count.into_wgpu(),
            dimension: descriptor.dimension.into_wgpu(),
            format: descriptor.format.into_wgpu(),
            usage: descriptor.usage.into_wgpu(),
            view_formats: &descriptor
                .view_formats
                .iter()
                .map(|&f: &TextureFormat| f.into_wgpu())
                .collect::<Vec<_>>(),
        };

        // Create the texture using the wgpu device with the specified descriptor
        let wgpu_texture = self.wgpu_device().create_texture(&wgpu_texture_descriptor);
        let id = self.generate_texture_id();
        let size_in_bytes = Self::calculate_texture_size_in_bytes(descriptor);

        // Track VRAM usage
        self.internal
            .vram_allocated_bytes
            .fetch_add(size_in_bytes as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the texture into the map
        self.with_registry(|r| {
            r.textures.insert(
                id,
                WgpuTextureEntry {
                    wgpu_texture: Arc::new(wgpu_texture),
                    size: size_in_bytes,
                },
            )
        })?;

        log::debug!(
            "WgpuDevice: Created texture '{:?}' with ID: {:?}, size: {} bytes (VRAM)",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id,
            size_in_bytes
        );
        Ok(id)
    }

    /// Removes a texture from the registry and gives its size back to the
    /// VRAM count.
    pub(super) fn release_texture(&self, id: api_tex::TextureId) -> Result<(), ResourceError> {
        // Remove the texture from the map and track VRAM usage
        if let Some(entry) = self.with_registry(|r| r.textures.remove(&id))? {
            self.internal
                .vram_allocated_bytes
                .fetch_sub(entry.size as usize, Ordering::Relaxed);
            log::debug!("WgpuDevice: Destroyed texture with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    /// Queues a write of `data` into a texture's base mip level.
    pub(super) fn queue_texture_write(
        &self,
        texture_id: api_tex::TextureId,
        data: &[u8],
        bytes_per_row: Option<u32>,
        offset: dimension::Origin3D,
        size: dimension::Extent3D,
    ) -> Result<(), ResourceError> {
        let wgpu_texture = self
            .with_registry(|r| {
                r.textures
                    .get(&texture_id)
                    .map(|e| Arc::clone(&e.wgpu_texture))
            })?
            .ok_or(ResourceError::NotFound)?;

        self.wgpu_queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &wgpu_texture,
                mip_level: 0, // Assuming base mip for now
                origin: offset.into_wgpu(),
                aspect: wgpu::TextureAspect::All, // Assuming all aspects for now
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row,
                rows_per_image: None, // Assuming 2D or 3D without specific rows per image info
            },
            size.into_wgpu(),
        );
        log::debug!(
            "WgpuDevice: Wrote {} bytes to texture ID: {:?} at offset {:?}",
            data.len(),
            texture_id,
            offset
        );
        Ok(())
    }

    /// Builds a view of a registered texture and registers it under a new
    /// id.
    pub(super) fn build_texture_view(
        &self,
        texture_id: api_tex::TextureId,
        descriptor: &api_tex::TextureViewDescriptor,
    ) -> Result<api_tex::TextureViewId, ResourceError> {
        let wgpu_texture = self
            .with_registry(|r| {
                r.textures
                    .get(&texture_id)
                    .map(|e| Arc::clone(&e.wgpu_texture))
            })?
            .ok_or(ResourceError::NotFound)?;

        let wgpu_view_descriptor = wgpu::TextureViewDescriptor {
            label: descriptor.label.as_deref(),
            format: descriptor.format.map(|f: TextureFormat| f.into_wgpu()),
            dimension: descriptor
                .dimension
                .map(|d: api_tex::TextureViewDimension| d.into_wgpu()),
            aspect: descriptor.aspect.into_wgpu(),
            base_mip_level: descriptor.base_mip_level,
            mip_level_count: descriptor.mip_level_count,
            base_array_layer: descriptor.base_array_layer,
            array_layer_count: descriptor.array_layer_count,
            usage: None,
        };

        // Create the texture view using the wgpu texture
        let wgpu_view = Arc::new(wgpu_texture.create_view(&wgpu_view_descriptor));
        let id = self.generate_texture_view_id();
        self.with_registry(|r| {
            r.texture_views.insert(
                id,
                WgpuTextureViewEntry {
                    wgpu_view,
                    source_texture_id: Some(texture_id),
                },
            )
        })?;
        log::info!(
            "WgpuDevice: Created texture view '{:?}' for texture ID: {:?} with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            texture_id,
            id
        );
        Ok(id)
    }

    /// Removes a texture view from the registry.
    pub(super) fn remove_texture_view(
        &self,
        id: api_tex::TextureViewId,
    ) -> Result<(), ResourceError> {
        // Remove the texture view from the map
        if self
            .with_registry(|r| r.texture_views.remove(&id))?
            .is_some()
        {
            log::debug!("WgpuDevice: Destroyed texture view with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    /// Builds a sampler and registers it under a new id.
    pub(super) fn build_sampler(
        &self,
        descriptor: &api_tex::SamplerDescriptor,
    ) -> Result<api_tex::SamplerId, ResourceError> {
        let wgpu_sampler_descriptor = wgpu::SamplerDescriptor {
            label: descriptor.label.as_deref(),
            address_mode_u: descriptor.address_mode_u.into_wgpu(),
            address_mode_v: descriptor.address_mode_v.into_wgpu(),
            address_mode_w: descriptor.address_mode_w.into_wgpu(),
            mag_filter: descriptor.mag_filter.into_wgpu(),
            min_filter: descriptor.min_filter.into_wgpu(),
            mipmap_filter: descriptor.mipmap_filter.into_wgpu(),
            lod_min_clamp: descriptor.lod_min_clamp,
            lod_max_clamp: descriptor.lod_max_clamp,
            compare: descriptor.compare.map(|f: CompareFunction| f.into_wgpu()),
            anisotropy_clamp: descriptor.anisotropy_clamp,
            border_color: descriptor
                .border_color
                .and_then(|c: api_tex::SamplerBorderColor| c.into_wgpu()),
        };

        // Create the sampler using the wgpu device
        let wgpu_sampler = Arc::new(self.wgpu_device().create_sampler(&wgpu_sampler_descriptor));
        let id = self.generate_sampler_id();
        self.with_registry(|r| r.samplers.insert(id, WgpuSamplerEntry { wgpu_sampler }))?;
        log::debug!(
            "WgpuDevice: Created sampler '{:?}' with ID: {:?}",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id
        );
        Ok(id)
    }

    /// Removes a sampler from the registry.
    pub(super) fn remove_sampler(&self, id: api_tex::SamplerId) -> Result<(), ResourceError> {
        // Remove the sampler from the map
        if self.with_registry(|r| r.samplers.remove(&id))?.is_some() {
            log::debug!("WgpuDevice: Destroyed sampler with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }
}
