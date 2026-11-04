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

//! Handing a lane's draw batches to the GPU.
//!
//! The lit lanes each produce two batches — the opaque draws, grouped by
//! pipeline, and the blended ones, sorted farthest-first. This module records
//! them, and owns the reason they are **two render passes into two command
//! buffers** rather than one loop.
//!
//! # Why the scene is split
//!
//! A blended surface must not write depth: two of them could not composite if
//! it did. So it leaves the pixel at the far plane — and the skybox paints
//! exactly the pixels left at the far plane. With the whole scene in one pass
//! the sky ran afterwards and repainted the glass, so a transparent sphere
//! against the sky vanished while the half of it overlapping the floor
//! survived, because there the floor had written depth.
//!
//! Splitting lets the engine fold the sky **between** the opaque draws that
//! give it a depth buffer to test against and the blended draws that composite
//! over it. See
//! [`TransparentPassSlot`](khora_data::render::TransparentPassSlot) for why the
//! alternative — drawing the sky first — was rejected on SAA grounds.

use khora_core::lane::io::SceneTarget;
use khora_core::math::LinearRgba;
use khora_core::renderer::api::command::{
    BindGroupId, DrawCommand, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::traits::{CommandEncoder, RenderPass};

/// The colour attachment of a scene pass into `target`, cleared or loaded.
pub(crate) fn scene_color(
    target: &SceneTarget,
    load: LoadOp<LinearRgba>,
) -> RenderPassColorAttachment<'_> {
    RenderPassColorAttachment {
        view: &target.color,
        resolve_target: None,
        ops: Operations {
            load,
            store: StoreOp::Store,
        },
        base_array_layer: 0,
        base_mip_level: 0,
    }
}

/// The depth attachment of a scene pass into `target`, cleared to the far
/// plane or loaded.
pub(crate) fn scene_depth(
    target: &SceneTarget,
    load: LoadOp<f32>,
) -> RenderPassDepthStencilAttachment<'_> {
    RenderPassDepthStencilAttachment {
        view: &target.depth,
        depth_ops: Some(Operations {
            load,
            store: StoreOp::Store,
        }),
        stencil_ops: None,
        base_array_layer: 0,
    }
}

/// Records a scene pass that only clears `target`: its colour to `clear`,
/// its depth to the far plane. A frame with no camera still shows the clear
/// colour.
pub(crate) fn record_clear(
    encoder: &mut dyn CommandEncoder,
    target: &SceneTarget,
    clear: LinearRgba,
    label: &'static str,
) {
    let color = [scene_color(target, LoadOp::Clear(clear))];
    let descriptor = RenderPassDescriptor {
        label: Some(label),
        color_attachments: &color,
        depth_stencil_attachment: Some(scene_depth(target, LoadOp::Clear(1.0))),
    };
    // Dropping the pass ends it: the clear is recorded without a draw.
    let _ = encoder.begin_render_pass(&descriptor);
}

/// Records a batch of draw commands into an open render pass.
///
/// Each lit lane calls it twice — once for the opaque batch, once for the
/// blended one, into two different passes. Setting the pipeline only when it
/// changes relies on the caller having sorted by pipeline (opaque) or by
/// depth (transparent, where correct compositing outranks batching).
pub(crate) fn record_draws<'a>(
    pass: &mut dyn RenderPass<'a>,
    commands: impl Iterator<Item = &'a DrawCommand>,
) {
    let mut current_pipeline: Option<RenderPipelineId> = None;
    for cmd in commands {
        if current_pipeline != Some(cmd.pipeline) {
            pass.set_pipeline(&cmd.pipeline);
            current_pipeline = Some(cmd.pipeline);
        }
        if let Some(ref bg) = cmd.model_bind_group {
            pass.set_bind_group(1, bg, &[cmd.model_offset]);
        }
        if let Some(ref bg) = cmd.material_bind_group {
            pass.set_bind_group(2, bg, &[]);
        }

        pass.set_vertex_buffer(0, &cmd.vertex_buffer, 0);
        pass.set_index_buffer(&cmd.index_buffer, 0, cmd.index_format);
        pass.draw_indexed(0..cmd.index_count, 0, 0..1);
    }
}

/// Records the scene's blended draws into the transparent pass's encoder,
/// loading the colour and the depth the opaque pass and the sky left.
///
/// Records nothing when there is nothing to blend: the pass is still closed,
/// empty, so its slot never carries a stale pass.
pub(crate) fn record_transparent_pass(
    encoder: &mut dyn CommandEncoder,
    transparent_draws: &[(f32, DrawCommand)],
    target: &SceneTarget,
    camera_bind_group: &BindGroupId,
    lighting_bind_group: &BindGroupId,
) {
    if transparent_draws.is_empty() {
        return;
    }
    let color = [scene_color(target, LoadOp::Load)];
    let descriptor = RenderPassDescriptor {
        label: Some("Scene Transparent Pass"),
        color_attachments: &color,
        depth_stencil_attachment: Some(scene_depth(target, LoadOp::Load)),
    };
    let mut pass = encoder.begin_render_pass(&descriptor);
    pass.set_bind_group(0, camera_bind_group, &[]);
    pass.set_bind_group(3, lighting_bind_group, &[]);
    record_draws(pass.as_mut(), transparent_draws.iter().map(|(_, cmd)| cmd));
}
