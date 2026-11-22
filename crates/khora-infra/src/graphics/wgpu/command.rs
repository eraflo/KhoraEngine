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
    BindGroupId, CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
};
use khora_core::renderer::api::pipeline::ComputePipelineId;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::buffer as api_buf;
use khora_core::renderer::api::resource::{IndexFormat, TextureId};
use khora_core::renderer::traits::{
    CommandEncoder, ComputePass, GpuProfiler, RenderPass, RenderPassError,
};
use std::any::Any;
use std::ops::Range;

use crate::graphics::wgpu::profiler::WgpuTimestampProfiler;

use super::conversions::IntoWgpu;
use super::device::WgpuDevice;

/// A render pass being recorded, resolving Khora ids through the device.
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
        bind_group_id: &'pass BindGroupId,
        dynamic_offsets: &[u32],
    ) {
        if let Some(bind_group) = self.device.get_wgpu_bind_group(*bind_group_id) {
            self.pass
                .set_bind_group(index, bind_group.as_ref(), dynamic_offsets);
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

    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.pass
            .set_viewport(x, y, width, height, min_depth, max_depth);
    }

    fn set_scissor_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        self.pass.set_scissor_rect(x, y, width, height);
    }
}

/// A compute pass being recorded, resolving Khora ids through the device.
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
        bind_group_id: &'pass BindGroupId,
        dynamic_offsets: &[u32],
    ) {
        if let Some(bind_group) = self.device.get_wgpu_bind_group(*bind_group_id) {
            self.pass
                .set_bind_group(index, bind_group.as_ref(), dynamic_offsets);
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

/// A wgpu command encoder behind the [`CommandEncoder`] trait.
pub struct WgpuCommandEncoder {
    pub(crate) encoder: Option<wgpu::CommandEncoder>,
    pub(crate) device: WgpuDevice,
}

impl WgpuCommandEncoder {
    /// Returns the underlying `wgpu::CommandEncoder`, or `None` once finished.
    ///
    /// An escape hatch for backend work the trait does not cover, such as
    /// timestamp profiling.
    pub(crate) fn wgpu_encoder_mut(&mut self) -> Option<&mut wgpu::CommandEncoder> {
        self.encoder.as_mut()
    }

    /// Returns the view a render pass draws into: `view`, remade as a 2D view of
    /// `layer` (and of `mip` alone, when given) when its source texture stands.
    ///
    /// A pass draws into a 2D view, never a 2D array, and this holds for layer 0
    /// too (the shadow atlas, a cube's faces).
    fn pass_view(
        &self,
        source: Option<TextureId>,
        view: &wgpu::TextureView,
        label: &'static str,
        layer: u32,
        mip: Option<u32>,
    ) -> wgpu::TextureView {
        match source.and_then(|id| self.device.get_wgpu_texture(id)) {
            Some(texture) => texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some(label),
                base_array_layer: layer,
                array_layer_count: Some(1),
                base_mip_level: mip.unwrap_or(0),
                mip_level_count: mip.map(|_| 1),
                dimension: Some(wgpu::TextureViewDimension::D2),
                ..Default::default()
            }),
            None => view.clone(),
        }
    }
}

impl CommandEncoder for WgpuCommandEncoder {
    fn begin_render_pass<'encoder>(
        &'encoder mut self,
        descriptor: &RenderPassDescriptor<'encoder>,
    ) -> Result<Box<dyn RenderPass<'encoder> + 'encoder>, RenderPassError> {
        // Every view is resolved before the pass begins, so a pass is begun
        // whole or refused. The views live here because the wgpu descriptor
        // borrows them.
        let attachments = descriptor.color_attachments;
        let mut views: Vec<wgpu::TextureView> = Vec::with_capacity(attachments.len());
        let mut resolve_targets: Vec<Option<wgpu::TextureView>> =
            Vec::with_capacity(attachments.len());
        for (index, att) in attachments.iter().enumerate() {
            let entry = self.device.get_wgpu_texture_view_entry(*att.view).ok_or(
                RenderPassError::UnknownColorView {
                    index,
                    view: *att.view,
                },
            )?;
            views.push(self.pass_view(
                entry.source_texture_id,
                &entry.wgpu_view,
                "Temp Layer View (Color)",
                att.base_array_layer,
                Some(att.base_mip_level),
            ));
            let resolve = match att.resolve_target {
                Some(target) => Some(
                    self.device
                        .get_wgpu_texture_view(target)
                        .map(|view| (*view).clone())
                        .ok_or(RenderPassError::UnknownResolveTarget {
                            index,
                            view: *target,
                        })?,
                ),
                None => None,
            };
            resolve_targets.push(resolve);
        }
        let depth_view = match &descriptor.depth_stencil_attachment {
            Some(ds) => {
                let entry = self
                    .device
                    .get_wgpu_texture_view_entry(*ds.view)
                    .ok_or(RenderPassError::UnknownDepthView { view: *ds.view })?;
                Some(self.pass_view(
                    entry.source_texture_id,
                    &entry.wgpu_view,
                    "Temp Layer View (Depth)",
                    ds.base_array_layer,
                    None,
                ))
            }
            None => None,
        };

        let color_attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = attachments
            .iter()
            .zip(&views)
            .zip(&resolve_targets)
            .map(|((att, view), resolve_target)| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: resolve_target.as_ref(),
                    ops: wgpu::Operations {
                        load: att.ops.load.clone().into_wgpu(),
                        store: att.ops.store.clone().into_wgpu(),
                    },
                    depth_slice: None,
                })
            })
            .collect();

        let depth_stencil_attachment = descriptor
            .depth_stencil_attachment
            .as_ref()
            .zip(depth_view.as_ref())
            .map(|(ds, view)| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: ds.depth_ops.as_ref().map(|ops| wgpu::Operations {
                    load: ops.load.clone().into_wgpu(),
                    store: ops.store.clone().into_wgpu(),
                }),
                stencil_ops: ds.stencil_ops.as_ref().map(|ops| wgpu::Operations {
                    load: ops.load.clone().into_wgpu(),
                    store: ops.store.clone().into_wgpu(),
                }),
            });

        let wgpu_descriptor = wgpu::RenderPassDescriptor {
            label: descriptor.label,
            color_attachments: &color_attachments,
            depth_stencil_attachment,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        };

        let pass = encoder_mut(&mut self.encoder).begin_render_pass(&wgpu_descriptor);

        Ok(Box::new(WgpuRenderPass {
            pass,
            device: &self.device,
        }))
    }

    fn begin_compute_pass<'encoder>(
        &'encoder mut self,
        descriptor: &ComputePassDescriptor<'encoder>,
    ) -> Box<dyn ComputePass<'encoder> + 'encoder> {
        let pass =
            encoder_mut(&mut self.encoder).begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: descriptor.label,
                // No timestamp writes on the abstract compute-pass entry point.
                // GPU timing is opt-in via `begin_profiler_compute_pass`, which
                // supplies a profiler's `ComputePassTimestampWrites`.
                timestamp_writes: None,
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
            other => {
                log::error!(
                    "begin_profiler_compute_pass: invalid profiler pass index {other}, defaulting to pass A"
                );
                concrete_profiler.compute_pass_a_timestamp_writes()
            }
        };

        let pass =
            encoder_mut(&mut self.encoder).begin_compute_pass(&wgpu::ComputePassDescriptor {
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
            encoder_mut(&mut self.encoder).copy_buffer_to_buffer(
                &source_buffer,
                source_offset,
                &destination_buffer,
                destination_offset,
                size,
            );
        }
    }

    fn finish(mut self: Box<Self>) -> Option<CommandBufferId> {
        let finished_encoder = self.encoder.take()?;
        self.device
            .register_command_buffer(finished_encoder.finish())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Returns the live encoder held in `slot`.
///
/// Takes the field rather than `&mut self`, so the returned pass can still
/// borrow the sibling `device`.
///
/// # Panics
///
/// After `finish()`, which no caller can reach: it consumes the boxed encoder.
fn encoder_mut(slot: &mut Option<wgpu::CommandEncoder>) -> &mut wgpu::CommandEncoder {
    slot.as_mut()
        .expect("command encoder used after finish() — passes must be created before finish()")
}
