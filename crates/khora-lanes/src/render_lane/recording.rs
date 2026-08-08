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

use khora_core::renderer::api::command::{
    BindGroupId, DrawCommand, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::core::RenderContext;
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::traits::{CommandEncoder, RenderPass};

/// Records a batch of draw commands into an open render pass.
///
/// The three lit lanes had this loop written out identically, and each now
/// calls it twice — once for the opaque batch, once for the blended one, into
/// two different passes. Setting the pipeline only when it changes relies on
/// the caller having sorted by pipeline (opaque) or by depth (transparent,
/// where correct compositing outranks batching).
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

/// Records the scene's blended draws into their own command buffer.
///
/// # Why this is not the same pass as the opaque draws
///
/// A blended surface must not write depth — two of them could not composite if
/// it did. So it leaves the pixel at the far plane, and the skybox paints
/// exactly the pixels left at the far plane. With one pass for the whole scene
/// the sky ran afterwards and repainted the glass: a transparent sphere against
/// the sky vanished, while the half of it overlapping the floor survived,
/// because there the floor had written depth.
///
/// Splitting the scene lets the engine fold the sky **between** the opaque
/// draws that give it a depth buffer to test against and the blended draws that
/// composite over it. See
/// [`TransparentPassSlot`](khora_data::render::TransparentPassSlot) for why the
/// alternative — drawing the sky first — was rejected on SAA grounds.
///
/// Does nothing when there is nothing to blend, so the common frame contributes
/// no second command buffer at all.
pub(crate) fn record_transparent_pass(
    encoder: Option<&mut (dyn CommandEncoder + '_)>,
    transparent_draws: &[(f32, DrawCommand)],
    render_ctx: &RenderContext,
    camera_bind_group: &BindGroupId,
    lighting_bind_group: &BindGroupId,
) {
    if transparent_draws.is_empty() {
        return;
    }

    let Some(encoder) = encoder else {
        // No second encoder: a caller that does not split its passes (a test
        // harness, or a lane driven outside `RenderAgent`). Blended geometry is
        // then not drawn at all rather than drawn in the wrong pass — the
        // failure that is visible rather than the one that looks almost right.
        log::debug!("record_transparent_pass: no transparent encoder, skipping blended draws");
        return;
    };

    // `Load` on both attachments: the opaque pass cleared and filled them, and
    // the sky has painted the background in between.
    let color_attachment = RenderPassColorAttachment {
        view: render_ctx.color_target,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
        base_array_layer: 0,
        base_mip_level: 0,
    };
    let descriptor = RenderPassDescriptor {
        label: Some("Scene Transparent Pass"),
        color_attachments: &[color_attachment],
        depth_stencil_attachment: render_ctx.depth_target.map(|depth_view| {
            RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
                base_array_layer: 0,
            }
        }),
    };

    let mut pass = encoder.begin_render_pass(&descriptor);
    pass.set_bind_group(0, camera_bind_group, &[]);
    pass.set_bind_group(3, lighting_bind_group, &[]);
    record_draws(pass.as_mut(), transparent_draws.iter().map(|(_, cmd)| cmd));
}
