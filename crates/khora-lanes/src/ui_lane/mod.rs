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

//! The UI lane: every UI node as one instanced quad, then the frame's text,
//! drawn over the frame's colour.
//!
//! The UI Flow lays the UI out and publishes it as a [`UiScene`]; the lane
//! uploads one instance per node, draws them as a triangle strip of four
//! vertices each, and hands the texts to the text renderer, which draws them
//! into the same encoder after the quads. Image nodes sample the shared
//! [`UiImageAtlas`], which the `image_on_gpu` residency fills before any
//! agent runs — the lane only reads it.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::io::{Target, TargetKey};
use khora_core::lane::level::{Impact, Level};
use khora_core::lane::LaneError;
use khora_core::math::{Extent2D, Extent3D, Mat4, Vec4};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutId, BindingResource, LoadOp,
    Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::{
    BufferUsage, ImageAspect, SamplerId, TextureDescriptor, TextureDimension, TextureFormat,
    TextureId, TextureUsage, TextureViewDescriptor, TextureViewDimension, TextureViewId,
};
use khora_core::renderer::api::text::TextRenderer;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{CommandEncoder, PipelineSystem};
use khora_core::renderer::GraphicsDevice;
use khora_data::ui::{UiImageAtlas, UiScene};
use khora_macros::LaneInputs;

use crate::render_lane::util::Block;

mod pipeline;
mod ui_family;

use pipeline::{
    ui_atlas_layout_entries, ui_global_layout_entries, ui_instance_layout_entries,
    ui_pipeline_spec, ui_sampler_descriptor, UI_ATLAS_LAYOUT_LABEL, UI_GLOBAL_LAYOUT_LABEL,
    UI_INSTANCE_LAYOUT_LABEL,
};

pub use ui_family::*;

/// UI nodes the instance storage holds before it first grows.
const UI_CAPACITY: usize = 1024;
/// The projection block's size: one 4×4 matrix.
const PROJECTION_BYTES: u64 = std::mem::size_of::<Mat4>() as u64;
/// The bytes the first build holds: the projection and the first storage.
const START_BYTES: u64 =
    PROJECTION_BYTES + (UI_CAPACITY * std::mem::size_of::<UiInstanceData>()) as u64;

/// What recording and uploading one node costs on the host.
const NODE_CPU: Duration = Duration::from_nanos(200);
/// What drawing one node's quad costs on the device.
const NODE_GPU: Duration = Duration::from_nanos(100);
/// What laying out and queueing one text costs on the host.
const TEXT_CPU: Duration = Duration::from_micros(2);
/// What drawing one text costs on the device.
const TEXT_GPU: Duration = Duration::from_micros(1);

/// Data for a single UI instance sent to the GPU — matches `UiInstance` in
/// `ui.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct UiInstanceData {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    params: [f32; 4],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
}

/// The UI lane. Holds what it built on the device; `None` before the first
/// prepare and after shutdown.
#[derive(Debug)]
pub struct UiLane {
    levels: [Level; 1],
    gpu: Option<UiGpu>,
}

impl Default for UiLane {
    fn default() -> Self {
        Self {
            levels: [Level {
                id: StrategyId::Balanced,
                name: "ui",
                impact: Impact::None,
                memory: PerDevice {
                    cpu: 0,
                    gpu: START_BYTES,
                    npu: 0,
                    other: 0,
                },
            }],
            gpu: None,
        }
    }
}

/// What the UI builds on the device.
#[derive(Debug)]
struct UiGpu {
    /// The projection matrix and its set-0 bind group.
    projection: Block,
    /// The node instances and their set-1 bind group, grown when a frame
    /// brings more nodes than they hold.
    instances: Block,
    instance_layout: BindGroupLayoutId,
    atlas_layout: BindGroupLayoutId,
    sampler: SamplerId,
    /// The pipeline and the colour format and samples it was built for.
    pipeline: Option<((TextureFormat, u32), RenderPipelineId)>,
    /// The set-2 bind group and the atlas view it binds, rebuilt only when
    /// the view changes.
    atlas_bind_group: Option<(TextureViewId, BindGroupId)>,
    /// A one-texel texture set 2 binds while the atlas has none: the layout
    /// needs a texture even when no node samples one.
    fallback: Option<(TextureId, TextureViewId)>,
}

impl UiGpu {
    /// The bytes its buffers hold.
    fn bytes(&self) -> u64 {
        self.projection.size + self.instances.size
    }

    fn destroy(self, device: &dyn GraphicsDevice) {
        if let Some((_, group)) = self.atlas_bind_group {
            if let Err(e) = device.destroy_bind_group(group) {
                log::warn!("UiLane: could not destroy the atlas bind group: {e:?}");
            }
        }
        if let Some((texture, view)) = self.fallback {
            destroy_texture(device, texture, view);
        }
        self.projection.destroy(device);
        self.instances.destroy(device);
        if let Err(e) = device.destroy_sampler(self.sampler) {
            log::warn!("UiLane: could not destroy the sampler: {e:?}");
        }
    }
}

/// What the UI lane reads each frame.
#[derive(LaneInputs)]
pub struct UiInputs<'f> {
    /// The frame's UI, laid out by the UI Flow.
    #[bus]
    pub scene: &'f UiScene,
    /// The frame's colour attachment and its size.
    #[frame]
    pub target: Target,
    /// The atlas image nodes sample, filled by the image residency.
    #[resource]
    pub atlas: &'f Arc<UiImageAtlas>,
    /// The renderer that draws the frame's texts.
    #[resource]
    pub text: &'f Arc<dyn TextRenderer>,
}

impl Lane for UiLane {
    type Family = UiFamily;
    type Inputs<'f> = UiInputs<'f>;

    fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Every node is one quad and every text one run of glyphs: their time on
    /// each device. An empty UI costs no time.
    fn cost(&self, _level: usize, inputs: &UiInputs<'_>) -> Cost {
        let nodes = inputs.scene.nodes.len() as u32;
        let texts = inputs.scene.texts.len() as u32;
        Cost {
            time: PerDevice {
                cpu: NODE_CPU * nodes + TEXT_CPU * texts,
                gpu: NODE_GPU * nodes + TEXT_GPU * texts,
                ..PerDevice::default()
            },
            memory: self.levels[0].memory,
            ..Cost::default()
        }
    }

    /// Builds the projection, the instance storage, their bind groups and
    /// the sampler once, and the pipeline for the target's colour format and
    /// samples: the UI tests no depth, so a depth change builds nothing.
    fn prepare(
        &mut self,
        _level: usize,
        gpu: &mut GpuSetup<'_>,
        key: Option<TargetKey>,
    ) -> Result<(), LaneError> {
        let device = gpu.device().ok_or(LaneError::missing("GraphicsDevice"))?;
        let pipelines = gpu
            .pipelines()
            .ok_or(LaneError::missing("PipelineSystem"))?;
        let built = match &mut self.gpu {
            Some(built) => built,
            empty => empty.insert(
                build_ui_gpu(device, pipelines)
                    .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
            ),
        };
        self.levels[0].memory.gpu = built.bytes();
        let wanted = key.map(|key| (key.color, key.samples));
        if built.pipeline.map(|(built_for, _)| built_for) != wanted {
            built.pipeline = match wanted {
                Some(formats) => Some((
                    formats,
                    pipelines
                        .pipeline(device, &ui_pipeline_spec(formats))
                        .map_err(|error| LaneError::InitializationFailed(Box::new(error)))?,
                )),
                None => None,
            };
        }
        Ok(())
    }

    /// Draws every node as a quad, then flushes the frame's texts over them,
    /// into the frame's colour. Everything the frame needs on the device is
    /// built and uploaded before the first text is queued, so a frame that
    /// fails leaves no text behind for the next one.
    fn record(
        &mut self,
        _level: usize,
        inputs: &mut UiInputs<'_>,
        out: &mut UiOutputs<'_>,
    ) -> Result<LaneReport, LaneError> {
        let built = self.gpu.as_mut().ok_or(LaneError::NotInitialized)?;
        let done = LaneReport {
            done: 1.0,
            stalled: false,
        };
        let scene = inputs.scene;
        if scene.nodes.is_empty() && scene.texts.is_empty() {
            return Ok(done);
        }
        let size = inputs.target.size;
        let Some(projection) = ui_projection(size) else {
            return Ok(done);
        };
        let device = out.pass.device();
        let failed = |error| LaneError::ExecutionFailed(Box::new(error));

        let instances = instances_of(scene, inputs.atlas);
        let needed = std::mem::size_of_val(instances.as_slice()) as u64;
        if needed > built.instances.size {
            let grown = grow_instances(built, device, needed);
            self.levels[0].memory.gpu = built.bytes();
            grown.map_err(failed)?;
        }
        let atlas_bind_group = if instances.is_empty() {
            None
        } else {
            Some(atlas_bind_group(built, device, inputs.atlas).map_err(failed)?)
        };
        device
            .write_buffer(built.projection.buffer, 0, bytemuck::bytes_of(&projection))
            .map_err(failed)?;
        if !instances.is_empty() {
            device
                .write_buffer(built.instances.buffer, 0, bytemuck::cast_slice(&instances))
                .map_err(failed)?;
        }

        let color = inputs.target.color;
        if let (Some(atlas_bind_group), Some((_, pipeline))) = (atlas_bind_group, built.pipeline) {
            draw_quads(
                out.pass.encoder(),
                color,
                pipeline,
                built,
                atlas_bind_group,
                instances.len() as u32,
            );
        }

        for text in &scene.texts {
            inputs
                .text
                .queue_text(text.layout.as_ref(), text.pos, text.color, text.z_index);
        }
        inputs
            .text
            .flush(device, out.pass.encoder(), &color, size)
            .map_err(LaneError::ExecutionFailed)?;
        Ok(done)
    }

    fn shutdown(&mut self, gpu: &mut GpuSetup<'_>) {
        if let (Some(built), Some(device)) = (self.gpu.take(), gpu.device()) {
            built.destroy(device);
        }
        self.levels[0].memory.gpu = START_BYTES;
    }
}

// ─── Free functions (CLAD: no inherent methods on the lane struct) ───

/// One instance per node, in the scene's order: its rectangle, colour
/// (white when it has none), corner radius, left border width, whether it
/// samples an image, and the image's rect in the atlas (the whole atlas for
/// an image not placed yet).
fn instances_of(scene: &UiScene, atlas: &UiImageAtlas) -> Vec<UiInstanceData> {
    scene
        .nodes
        .iter()
        .map(|node| {
            let color = node.color.map(|c| c.0).unwrap_or(Vec4::ONE);
            let radius = node.border.map(|b| b.radius).unwrap_or(0.0);
            let border = node.border.map(|b| b.width.left.max(0.0)).unwrap_or(0.0);
            let has_image = if node.image.is_some() { 1.0 } else { 0.0 };
            let (uv_min, uv_max) = node
                .image
                .and_then(|image| atlas.get_rect(&image.texture))
                .map(|rect| (rect.min.into(), rect.max.into()))
                .unwrap_or(([0.0, 0.0], [1.0, 1.0]));
            UiInstanceData {
                pos: node.pos.into(),
                size: node.size.into(),
                color: color.into(),
                params: [radius, border, has_image, 0.0],
                uv_min,
                uv_max,
            }
        })
        .collect()
}

/// The layouts, the projection and the first instance storage, and the
/// sampler. A refusal halfway gives back what came before it.
fn build_ui_gpu(
    device: &dyn GraphicsDevice,
    pipelines: &dyn PipelineSystem,
) -> Result<UiGpu, RenderError> {
    // Layouts belong to the PipelineSystem, which caches them.
    let global_layout =
        pipelines.inline_layout(device, UI_GLOBAL_LAYOUT_LABEL, &ui_global_layout_entries())?;
    let instance_layout = pipelines.inline_layout(
        device,
        UI_INSTANCE_LAYOUT_LABEL,
        &ui_instance_layout_entries(),
    )?;
    let atlas_layout =
        pipelines.inline_layout(device, UI_ATLAS_LAYOUT_LABEL, &ui_atlas_layout_entries())?;

    let projection = Block::new(
        device,
        global_layout,
        PROJECTION_BYTES,
        BufferUsage::UNIFORM,
        "ui_projection",
    )
    .map_err(RenderError::ResourceError)?;
    let instances = match instance_block(device, instance_layout, UI_CAPACITY) {
        Ok(instances) => instances,
        Err(error) => {
            projection.destroy(device);
            return Err(RenderError::ResourceError(error));
        }
    };
    let sampler = match device.create_sampler(&ui_sampler_descriptor()) {
        Ok(sampler) => sampler,
        Err(error) => {
            instances.destroy(device);
            projection.destroy(device);
            return Err(RenderError::ResourceError(error));
        }
    };
    Ok(UiGpu {
        projection,
        instances,
        instance_layout,
        atlas_layout,
        sampler,
        pipeline: None,
        atlas_bind_group: None,
        fallback: None,
    })
}

/// An instance storage holding `nodes` instances.
fn instance_block(
    device: &dyn GraphicsDevice,
    layout: BindGroupLayoutId,
    nodes: usize,
) -> Result<Block, ResourceError> {
    Block::new(
        device,
        layout,
        (nodes * std::mem::size_of::<UiInstanceData>()) as u64,
        BufferUsage::STORAGE,
        "ui_instances",
    )
}

/// Replaces the instance storage by one holding at least `needed` bytes,
/// twice the nodes it held at least; the old one is given back once the new
/// stands.
fn grow_instances(
    built: &mut UiGpu,
    device: &dyn GraphicsDevice,
    needed: u64,
) -> Result<(), ResourceError> {
    let instance = std::mem::size_of::<UiInstanceData>() as u64;
    let held = built.instances.size / instance;
    let nodes = needed.div_ceil(instance).max(held * 2) as usize;
    let grown = instance_block(device, built.instance_layout, nodes)?;
    std::mem::replace(&mut built.instances, grown).destroy(device);
    Ok(())
}

/// The set-2 bind group over the atlas's view, or over a one-texel fallback
/// while the atlas has no texture. It is kept while the view it binds stays
/// the same, and the one it replaces is given back.
fn atlas_bind_group(
    built: &mut UiGpu,
    device: &dyn GraphicsDevice,
    atlas: &UiImageAtlas,
) -> Result<BindGroupId, ResourceError> {
    let atlas_view = atlas
        .lock_atlas()
        .and_then(|guard| guard.as_ref().map(|atlas| atlas.view()));
    let view = match atlas_view {
        Some(view) => view,
        None => match built.fallback {
            Some((_, view)) => view,
            None => {
                let fallback = fallback_texture(device)?;
                built.fallback = Some(fallback);
                fallback.1
            }
        },
    };
    if let Some((bound, group)) = built.atlas_bind_group {
        if bound == view {
            return Ok(group);
        }
    }
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("ui_atlas_bind_group"),
        layout: built.atlas_layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(view),
                _phantom: std::marker::PhantomData,
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(built.sampler),
                _phantom: std::marker::PhantomData,
            },
        ],
    })?;
    if let Some((_, stale)) = built.atlas_bind_group.replace((view, group)) {
        if let Err(e) = device.destroy_bind_group(stale) {
            log::warn!("UiLane: could not destroy the previous atlas bind group: {e:?}");
        }
    }
    Ok(group)
}

/// A one-texel texture and its view, for set 2 while the atlas has none.
fn fallback_texture(
    device: &dyn GraphicsDevice,
) -> Result<(TextureId, TextureViewId), ResourceError> {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Borrowed("ui_atlas_fallback")),
        size: Extent3D {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: khora_core::renderer::api::resource::SampleCount::X1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsage::TEXTURE_BINDING,
        view_formats: Cow::Borrowed(&[]),
    })?;
    let view = device.create_texture_view(
        texture,
        &TextureViewDescriptor {
            label: Some(Cow::Borrowed("ui_atlas_fallback_view")),
            format: None,
            dimension: Some(TextureViewDimension::D2),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        },
    );
    match view {
        Ok(view) => Ok((texture, view)),
        Err(error) => {
            if let Err(e) = device.destroy_texture(texture) {
                log::warn!("UiLane: could not destroy the fallback texture: {e:?}");
            }
            Err(error)
        }
    }
}

fn destroy_texture(device: &dyn GraphicsDevice, texture: TextureId, view: TextureViewId) {
    if let Err(e) = device.destroy_texture_view(view) {
        log::warn!("UiLane: could not destroy the fallback view: {e:?}");
    }
    if let Err(e) = device.destroy_texture(texture) {
        log::warn!("UiLane: could not destroy the fallback texture: {e:?}");
    }
}

/// The quad pass — `LoadOp::Load` preserves what the scene drew; no depth.
fn draw_quads(
    encoder: &mut dyn CommandEncoder,
    color: TextureViewId,
    pipeline: RenderPipelineId,
    built: &UiGpu,
    atlas_bind_group: BindGroupId,
    nodes: u32,
) {
    let color_attachment = RenderPassColorAttachment {
        view: &color,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        },
        base_array_layer: 0,
        base_mip_level: 0,
    };
    let attachments = [color_attachment];
    let pass_desc = RenderPassDescriptor {
        label: Some("UI Render Pass"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
    };
    let mut pass = encoder.begin_render_pass(&pass_desc);
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &built.projection.bind_group, &[]);
    pass.set_bind_group(1, &built.instances.bind_group, &[]);
    pass.set_bind_group(2, &atlas_bind_group, &[]);
    // Four vertices per instance: a quad drawn as a triangle strip.
    pass.draw(0..4, 0..nodes);
}

/// The orthographic projection that maps UI pixel coordinates of `target`
/// (origin top-left, y down) to clip space; `None` when a side is zero.
pub(crate) fn ui_projection(target: Extent2D) -> Option<Mat4> {
    if target.width == 0 || target.height == 0 {
        return None;
    }
    Some(Mat4::orthographic_rh_zo(
        0.0,
        target.width as f32,
        target.height as f32,
        0.0,
        0.0,
        1.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_instances_are_sixty_four_bytes() {
        // Sixteen floats, the stride of `UiInstance` in `ui.wgsl`.
        assert_eq!(std::mem::size_of::<UiInstanceData>(), 64);
    }

    #[test]
    fn a_target_without_area_has_no_projection() {
        assert!(ui_projection(Extent2D {
            width: 0,
            height: 600
        })
        .is_none());
        assert!(ui_projection(Extent2D {
            width: 800,
            height: 600
        })
        .is_some());
    }
}
