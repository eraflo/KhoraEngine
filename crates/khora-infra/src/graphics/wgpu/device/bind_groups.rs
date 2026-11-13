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

//! The wgpu device's bind group layouts and bind groups: building them from
//! the engine's descriptors over registered resources, removing them.

use khora_core::renderer::api::command as api_cmd;
use khora_core::renderer::api::command::{BindGroupId, BindGroupLayoutId};
use khora_core::renderer::ResourceError;

use crate::graphics::wgpu::conversions::IntoWgpu;

use std::sync::Arc;
use wgpu;

use super::entries::{WgpuBindGroupEntry, WgpuBindGroupLayoutEntry};
use super::WgpuDevice;

impl WgpuDevice {
    /// Builds a bind group layout and registers it under a new id.
    pub(super) fn build_bind_group_layout(
        &self,
        descriptor: &api_cmd::BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        use api_cmd::{BindingType, BufferBindingType};

        let wgpu_entries: Vec<wgpu::BindGroupLayoutEntry> = descriptor
            .entries
            .iter()
            .map(|entry| {
                let ty = match &entry.ty {
                    BindingType::Buffer {
                        ty,
                        has_dynamic_offset,
                        min_binding_size,
                    } => {
                        let buffer_ty = match ty {
                            BufferBindingType::Uniform => wgpu::BufferBindingType::Uniform,
                            BufferBindingType::Storage { read_only } => {
                                wgpu::BufferBindingType::Storage {
                                    read_only: *read_only,
                                }
                            }
                        };
                        wgpu::BindingType::Buffer {
                            ty: buffer_ty,
                            has_dynamic_offset: *has_dynamic_offset,
                            min_binding_size: *min_binding_size,
                        }
                    }
                    BindingType::Texture {
                        sample_type,
                        view_dimension,
                        multisampled,
                    } => wgpu::BindingType::Texture {
                        sample_type: (*sample_type).into_wgpu(),
                        view_dimension: (*view_dimension).into_wgpu(),
                        multisampled: *multisampled,
                    },
                    BindingType::Sampler(sampler_ty) => {
                        wgpu::BindingType::Sampler((*sampler_ty).into_wgpu())
                    }
                };

                wgpu::BindGroupLayoutEntry {
                    binding: entry.binding,
                    visibility: entry.visibility.into_wgpu(),
                    ty,
                    count: None,
                }
            })
            .collect();

        let wgpu_layout = self.with_wgpu_device(|device| {
            let wgpu_descriptor = wgpu::BindGroupLayoutDescriptor {
                label: descriptor.label,
                entries: &wgpu_entries,
            };
            Ok(Arc::new(device.create_bind_group_layout(&wgpu_descriptor)))
        })?;

        let id = self.generate_bind_group_layout_id();
        self.with_registry(|r| {
            r.bind_group_layouts
                .insert(id, WgpuBindGroupLayoutEntry { wgpu_layout })
        })?;

        log::debug!(
            "WgpuDevice: Created bind group layout '{:?}' with ID: {:?}",
            descriptor.label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    /// Builds a bind group over registered buffers, texture views and
    /// samplers, and registers it under a new id.
    pub(super) fn build_bind_group(
        &self,
        descriptor: &api_cmd::BindGroupDescriptor,
    ) -> Result<BindGroupId, ResourceError> {
        use api_cmd::BindingResource;

        // Resource storage to hold Arcs alive
        enum ResourceArc {
            Buffer(u32, Arc<wgpu::Buffer>, u64, Option<std::num::NonZeroU64>),
            TextureView(u32, Arc<wgpu::TextureView>),
            Sampler(u32, Arc<wgpu::Sampler>),
        }

        // The layout and every bound resource, cloned out of the registry in
        // one pass.
        let (wgpu_layout, resource_arcs) = self
            .with_registry(|r| {
                let layout = r
                    .bind_group_layouts
                    .get(&descriptor.layout)
                    .map(|e| Arc::clone(&e.wgpu_layout))?;
                let resources = descriptor
                    .entries
                    .iter()
                    .map(|entry| match &entry.resource {
                        BindingResource::Buffer(binding) => {
                            r.buffers.get(&binding.buffer).map(|e| {
                                ResourceArc::Buffer(
                                    entry.binding,
                                    Arc::clone(&e.wgpu_buffer),
                                    binding.offset,
                                    binding.size,
                                )
                            })
                        }
                        BindingResource::TextureView(view_id) => {
                            r.texture_views.get(view_id).map(|e| {
                                ResourceArc::TextureView(entry.binding, Arc::clone(&e.wgpu_view))
                            })
                        }
                        BindingResource::Sampler(sampler_id) => {
                            r.samplers.get(sampler_id).map(|e| {
                                ResourceArc::Sampler(entry.binding, Arc::clone(&e.wgpu_sampler))
                            })
                        }
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some((layout, resources))
            })?
            .ok_or(ResourceError::NotFound)?;

        // Now create wgpu entries with references to the Arc'd resources
        let wgpu_entries: Vec<wgpu::BindGroupEntry> = resource_arcs
            .iter()
            .map(|resource| match resource {
                ResourceArc::Buffer(binding, buffer, offset, size) => wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: buffer.as_ref(),
                        offset: *offset,
                        size: *size,
                    }),
                },
                ResourceArc::TextureView(binding, view) => wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::TextureView(view.as_ref()),
                },
                ResourceArc::Sampler(binding, sampler) => wgpu::BindGroupEntry {
                    binding: *binding,
                    resource: wgpu::BindingResource::Sampler(sampler.as_ref()),
                },
            })
            .collect();

        let wgpu_bind_group = self.with_wgpu_device(|device| {
            let wgpu_descriptor = wgpu::BindGroupDescriptor {
                label: descriptor.label,
                layout: &wgpu_layout,
                entries: &wgpu_entries,
            };
            Ok(Arc::new(device.create_bind_group(&wgpu_descriptor)))
        })?;

        let id = self.generate_bind_group_id();
        self.with_registry(|r| {
            r.bind_groups
                .insert(id, WgpuBindGroupEntry { wgpu_bind_group })
        })?;

        log::debug!(
            "WgpuDevice: Created bind group '{:?}' with ID: {:?}",
            descriptor.label.unwrap_or_default(),
            id
        );
        Ok(id)
    }

    /// Removes a bind group layout from the registry.
    pub(super) fn remove_bind_group_layout(
        &self,
        id: BindGroupLayoutId,
    ) -> Result<(), ResourceError> {
        if self
            .with_registry(|r| r.bind_group_layouts.remove(&id))?
            .is_some()
        {
            log::debug!("WgpuDevice: Destroyed bind group layout with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    /// Removes a bind group from the registry.
    pub(super) fn remove_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        if self.with_registry(|r| r.bind_groups.remove(&id))?.is_some() {
            log::debug!("WgpuDevice: Destroyed bind group with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }
}
