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

use khora_core::renderer::api::command::{
    CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::traits::{CommandEncoder, ComputePass, GpuProfiler, RenderPass};
use khora_core::renderer::{
    api::buffer as api_buf, ComputePipelineId, IndexFormat, RenderPipelineId,
};
use std::any::Any;
use std::ops::Range;

use crate::graphics::wgpu::profiler::WgpuTimestampProfiler;

use super::conversions::IntoWgpu;
use super::device::WgpuDevice;

// --- Implémentations concrètes ---

pub struct WgpuRenderPass<'a> {
    pub(crate) pass: wgpu::RenderPass<'a>,
    pub(crate) device: &'a WgpuDevice,
}

impl<'pass> RenderPass<'pass> for WgpuRenderPass<'pass> {
    fn set_pipeline(&mut self, pipeline_id: &'pass RenderPipelineId) {
        if let Some(pipeline) = self.device.get_wgpu_render_pipeline(*pipeline_id) {
            self.pass.set_pipeline(&pipeline);
        } else {
            log::warn!(
                "WgpuRenderPass: RenderPipelineId {:?} not found.",
                pipeline_id
            );
        }
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        bind_group_id: &'pass khora_core::renderer::BindGroupId,
    ) {
        if let Some(bind_group) = self.device.get_wgpu_bind_group(*bind_group_id) {
            self.pass.set_bind_group(index, bind_group.as_ref(), &[]);
        } else {
            log::warn!("WgpuRenderPass: BindGroupId {:?} not found.", bind_group_id);
        }
    }

    fn set_vertex_buffer(&mut self, slot: u32, buffer_id: &'pass api_buf::BufferId, offset: u64) {
        if let Some(buffer) = self.device.get_wgpu_buffer(*buffer_id) {
            self.pass.set_vertex_buffer(slot, buffer.slice(offset..));
        } else {
            log::warn!("WgpuRenderPass: Vertex BufferId {:?} not found.", buffer_id);
        }
    }

    fn set_index_buffer(
        &mut self,
        buffer_id: &'pass api_buf::BufferId,
        offset: u64,
        index_format: IndexFormat,
    ) {
        if let Some(buffer) = self.device.get_wgpu_buffer(*buffer_id) {
            self.pass
                .set_index_buffer(buffer.slice(offset..), index_format.into_wgpu());
        } else {
            log::warn!("WgpuRenderPass: Index BufferId {:?} not found.", buffer_id);
        }
    }

    fn draw(&mut self, vertices: Range<u32>, instances: Range<u32>) {
        self.pass.draw(vertices, instances);
    }

    fn draw_indexed(&mut self, indices: Range<u32>, base_vertex: i32, instances: Range<u32>) {
        self.pass.draw_indexed(indices, base_vertex, instances);
    }
}

pub struct WgpuComputePass<'a> {
    pub(crate) pass: wgpu::ComputePass<'a>,
    pub(crate) device: &'a WgpuDevice,
}

impl<'pass> ComputePass<'pass> for WgpuComputePass<'pass> {
    fn set_pipeline(&mut self, pipeline_id: &'pass ComputePipelineId) {
        if let Some(pipeline) = self.device.get_wgpu_compute_pipeline(*pipeline_id) {
            self.pass.set_pipeline(&pipeline);
        } else {
            log::warn!(
                "WgpuComputePass: ComputePipelineId {:?} not found.",
                pipeline_id
            );
        }
    }

    fn set_bind_group(
        &mut self,
        index: u32,
        bind_group_id: &'pass khora_core::renderer::BindGroupId,
    ) {
        if let Some(bind_group) = self.device.get_wgpu_bind_group(*bind_group_id) {
            self.pass.set_bind_group(index, bind_group.as_ref(), &[]);
        } else {
            log::warn!(
                "WgpuComputePass: BindGroupId {:?} not found.",
                bind_group_id
            );
        }
    }

    fn dispatch_workgroups(&mut self, x: u32, y: u32, z: u32) {
        self.pass.dispatch_workgroups(x, y, z);
    }
}

pub struct WgpuCommandEncoder {
    pub(crate) encoder: Option<wgpu::CommandEncoder>,
    pub(crate) device: WgpuDevice,
}

impl WgpuCommandEncoder {
    /// Provides mutable access to the underlying `wgpu::CommandEncoder`.
    /// This is an "escape hatch" for backend-specific operations like
    /// timestamp profiling that are not fully abstracted yet.
    /// Returns `None` if the encoder has already been consumed by `finish()`.
    pub fn wgpu_encoder_mut(&mut self) -> Option<&mut wgpu::CommandEncoder> {
        self.encoder.as_mut()
    }
}

impl CommandEncoder for WgpuCommandEncoder {
    fn begin_render_pass<'encoder>(
        &'encoder mut self,
        descriptor: &RenderPassDescriptor<'encoder>,
    ) -> Box<dyn RenderPass<'encoder> + 'encoder> {
        // Collect all views and resolve targets first to ensure their lifetimes are valid
        let mut views: Vec<wgpu::TextureView> = Vec::new();
        let mut resolve_targets: Vec<Option<wgpu::TextureView>> = Vec::new();

        for att in descriptor.color_attachments.iter() {
            if let Some(view) = self.device.get_wgpu_texture_view(att.view) {
                views.push((*view).clone());
            } else {
                // Push a dummy view if not found (should handle error properly)
                // For now, skip this attachment
                continue;
            }
            if let Some(rt_id) = att.resolve_target {
                resolve_targets.push(
                    self.device
                        .get_wgpu_texture_view(rt_id)
                        .map(|arc_view| (*arc_view).clone()),
                );
            } else {
                resolve_targets.push(None);
            }
        }

        let color_attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = descriptor
            .color_attachments
            .iter()
            .enumerate()
            .map(|(i, att)| {
                Some(wgpu::RenderPassColorAttachment {
                    view: &views[i],
                    resolve_target: resolve_targets[i].as_ref(),
                    ops: wgpu::Operations {
                        load: att.ops.load.clone().into_wgpu(),
                        store: att.ops.store.clone().into_wgpu(),
                    },
                    depth_slice: None,
                })
            })
            .collect();

        // Handle depth/stencil attachment
        let depth_view: Option<wgpu::TextureView> =
            descriptor.depth_stencil_attachment.as_ref().and_then(|ds| {
                self.device
                    .get_wgpu_texture_view(ds.view)
                    .map(|arc_view| (*arc_view).clone())
            });

        let depth_stencil_attachment = match (&descriptor.depth_stencil_attachment, &depth_view) {
            (Some(ds), Some(view)) => Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: ds.depth_ops.as_ref().map(|ops| wgpu::Operations {
                    load: ops.load.clone().into_wgpu(),
                    store: ops.store.clone().into_wgpu(),
                }),
                stencil_ops: ds.stencil_ops.as_ref().map(|ops| wgpu::Operations {
                    load: ops.load.clone().into_wgpu(),
                    store: ops.store.clone().into_wgpu(),
                }),
            }),
            _ => None,
        };

        let wgpu_descriptor = wgpu::RenderPassDescriptor {
            label: descriptor.label,
            color_attachments: &color_attachments,
            depth_stencil_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        };

        let pass = self
            .encoder
            .as_mut()
            .unwrap()
            .begin_render_pass(&wgpu_descriptor);

        Box::new(WgpuRenderPass {
            pass,
            device: &self.device,
        })
    }

    fn begin_compute_pass<'encoder>(
        &'encoder mut self,
        descriptor: &ComputePassDescriptor<'encoder>,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder> {
        let pass =
            self.encoder
                .as_mut()
                .unwrap()
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: descriptor.label,
                    timestamp_writes: None, // TODO
                });

        Box::new(WgpuComputePass {
            pass,
            device: &self.device,
        })
    }

    fn begin_profiler_compute_pass<'encoder>(
        &'encoder mut self,
        label: Option<&str>,
        profiler: &'encoder dyn GpuProfiler,
        pass_index: u32, // 0 for A, 1 for B
    ) -> Box<dyn ComputePass<'encoder> + 'encoder> {
        let concrete_profiler = profiler
            .as_any()
            .downcast_ref::<WgpuTimestampProfiler>()
            .expect("GpuProfiler must be a WgpuTimestampProfiler for the WGPU backend.");

        let timestamp_writes = match pass_index {
            0 => concrete_profiler.compute_pass_a_timestamp_writes(),
            1 => concrete_profiler.compute_pass_b_timestamp_writes(),
            _ => panic!("Invalid profiler pass index"),
        };

        let pass =
            self.encoder
                .as_mut()
                .unwrap()
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label,
                    timestamp_writes: Some(timestamp_writes),
                });

        Box::new(WgpuComputePass {
            pass,
            device: &self.device,
        })
    }

    fn copy_buffer_to_buffer(
        &mut self,
        source: &api_buf::BufferId,
        source_offset: u64,
        destination: &api_buf::BufferId,
        destination_offset: u64,
        size: u64,
    ) {
        if let (Some(source_buffer), Some(destination_buffer)) = (
            self.device.get_wgpu_buffer(*source),
            self.device.get_wgpu_buffer(*destination),
        ) {
            self.encoder.as_mut().unwrap().copy_buffer_to_buffer(
                &source_buffer,
                source_offset,
                &destination_buffer,
                destination_offset,
                size,
            );
        }
    }

    fn finish(mut self: Box<Self>) -> CommandBufferId {
        let finished_encoder = self.encoder.take().unwrap();
        self.device
            .register_command_buffer(finished_encoder.finish())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
