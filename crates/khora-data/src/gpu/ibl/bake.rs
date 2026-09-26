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

//! Recording the bake: the sky capture, irradiance convolution, specular
//! prefilter and BRDF passes.

use super::pipelines::{
    brdf_pipeline_spec, equirect_layout_entries, equirect_pipeline_spec, irradiance_layout_entries,
    irradiance_pipeline_spec, prefilter_pipeline_spec, sky_pipeline_spec,
};
use super::resources::{
    create_brdf_lut, create_cube, create_equirect_sampler, create_face_basis_buffers,
    create_ibl_sampler, create_mip_cube, create_prefilter_basis_buffers, uniform_entry,
    upload_equirect, IblResources,
};
use super::{
    ENV_FACE_SIZE, EQUIRECT_LAYOUT, FACE_BASIS_LAYOUT, IRRADIANCE_FACE_SIZE, IRRADIANCE_LAYOUT,
    PREFILTER_FACE_SIZE, PREFILTER_LAYOUT, PREFILTER_MIPS,
};
use khora_core::math::{LinearRgba, Vec3};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindingResource, BufferBinding, LoadOp, Operations,
    RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::ibl::IblGpuBindings;
use khora_core::renderer::api::resource::{BufferId, SamplerId, TextureId, TextureViewId};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

/// Runs the full one-time bake: env cube → diffuse irradiance cube → prefiltered
/// specular cube + BRDF LUT, plus the shared sampler.
///
/// The env cube is filled either by projecting an authored equirectangular map
/// (`env_source`) or by the procedural sky. Everything downstream reads the
/// cube and is identical in both cases.
pub(super) fn bake(
    device: &dyn GraphicsDevice,
    pipeline_system: &dyn PipelineSystem,
    sun: Vec3,
    env_source: Option<&khora_core::renderer::api::resource::CpuTexture>,
) -> Result<IblResources, RenderError> {
    let env = create_cube(device, ENV_FACE_SIZE, "IBL Env")?;
    let irradiance = create_cube(device, IRRADIANCE_FACE_SIZE, "IBL Irradiance")?;
    let (prefilter_texture, prefilter_view) =
        create_mip_cube(device, PREFILTER_FACE_SIZE, PREFILTER_MIPS, "IBL Prefilter")?;
    let (brdf_texture, brdf_view) = create_brdf_lut(device)?;
    let sampler = create_ibl_sampler(device)?;

    // Pipelines + inline layouts.
    let irr_pipeline = pipeline_system.pipeline(device, &irradiance_pipeline_spec())?;
    let irr_layout =
        pipeline_system.inline_layout(device, IRRADIANCE_LAYOUT, &irradiance_layout_entries())?;
    let pre_pipeline = pipeline_system.pipeline(device, &prefilter_pipeline_spec())?;
    let pre_layout =
        pipeline_system.inline_layout(device, PREFILTER_LAYOUT, &irradiance_layout_entries())?;
    let brdf_pipeline = pipeline_system.pipeline(device, &brdf_pipeline_spec())?;

    let sky_bufs = create_face_basis_buffers(device, sun)?;
    let irr_bufs = create_face_basis_buffers(device, sun)?;

    // Environment source — an authored equirectangular map when the scene
    // supplies one, else the procedural sky. Both paths write the same six env
    // cube faces with the same per-face basis uniforms.
    let mut equirect_keep: Option<(TextureId, TextureViewId)> = None;
    let (env_pipeline, env_bgs) = match env_source {
        Some(cpu) => {
            let (texture, view) = upload_equirect(device, cpu)?;
            equirect_keep = Some((texture, view));
            let equirect_sampler = create_equirect_sampler(device)?;
            let pipeline = pipeline_system.pipeline(device, &equirect_pipeline_spec())?;
            let layout = pipeline_system.inline_layout(
                device,
                EQUIRECT_LAYOUT,
                &equirect_layout_entries(),
            )?;
            let mut bgs = Vec::with_capacity(6);
            for buf in &sky_bufs {
                bgs.push(sampled_texture_bind_group(
                    device,
                    layout,
                    view,
                    equirect_sampler,
                    *buf,
                )?);
            }
            log::info!(
                "IBL: environment from authored equirectangular map ({}x{}, {:?})",
                cpu.size.width,
                cpu.size.height,
                cpu.format
            );
            (pipeline, bgs)
        }
        None => {
            let pipeline = pipeline_system.pipeline(device, &sky_pipeline_spec())?;
            let layout =
                pipeline_system.inline_layout(device, FACE_BASIS_LAYOUT, &[uniform_entry(0)])?;
            let mut bgs = Vec::with_capacity(6);
            for buf in &sky_bufs {
                bgs.push(device.create_bind_group(&BindGroupDescriptor {
                    label: Some("IBL Sky Face BG"),
                    layout,
                    entries: &[uniform_bg_entry(0, *buf)],
                })?);
            }
            (pipeline, bgs)
        }
    };
    // Irradiance bind groups (env cube + sampler + uniform), one per face.
    let mut irr_bgs = Vec::with_capacity(6);
    for buf in &irr_bufs {
        irr_bgs.push(sampled_texture_bind_group(
            device,
            irr_layout,
            env.cube_view,
            sampler,
            *buf,
        )?);
    }
    // Prefilter bind groups — one per (mip, face); roughness rises with the mip.
    let mut pre_bufs: Vec<BufferId> = Vec::with_capacity((PREFILTER_MIPS * 6) as usize);
    let mut pre_bgs = Vec::with_capacity((PREFILTER_MIPS * 6) as usize);
    for mip in 0..PREFILTER_MIPS {
        // Roughness spans [0, 1] across the mip chain (PREFILTER_MIPS >= 2).
        let roughness = mip as f32 / (PREFILTER_MIPS - 1) as f32;
        let bufs = create_prefilter_basis_buffers(device, roughness)?;
        for buf in &bufs {
            pre_bgs.push(sampled_texture_bind_group(
                device,
                pre_layout,
                env.cube_view,
                sampler,
                *buf,
            )?);
        }
        pre_bufs.extend(bufs);
    }

    // Submission 1: sky → env cube. The convolution + prefilter SAMPLE the env
    // cube, so it must fully complete first; cross-submission ordering on the
    // queue guarantees that (a single encoder would leave the write→read hazard
    // unsynchronised — nothing else in the engine writes then reads a texture
    // within one encoder).
    let mut sky_encoder = device.create_command_encoder(Some("IBL Env Bake"));
    record_face_passes(
        &mut *sky_encoder,
        &env_pipeline,
        &env.face_views,
        &env_bgs,
        "IBL Env Face",
    );
    match sky_encoder.finish() {
        Some(cb) => device.submit_command_buffer(cb),
        None => log::error!("IBL: sky bake encoder finish returned None; skipping submit"),
    }

    // Submission 2: irradiance + prefiltered specular (both read env) + the
    // environment-independent BRDF LUT.
    let mut encoder = device.create_command_encoder(Some("IBL Filter Bake"));
    record_face_passes(
        &mut *encoder,
        &irr_pipeline,
        &irradiance.face_views,
        &irr_bgs,
        "IBL Irradiance Face",
    );
    // Prefilter: one pass per (mip, face), each writing that mip's roughness.
    for mip in 0..PREFILTER_MIPS {
        for face in 0..6usize {
            let idx = (mip as usize) * 6 + face;
            let attachments = [RenderPassColorAttachment {
                view: &prefilter_view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(LinearRgba::BLACK),
                    store: StoreOp::Store,
                },
                base_array_layer: face as u32,
                base_mip_level: mip,
            }];
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("IBL Prefilter Face"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
            });
            pass.set_pipeline(&pre_pipeline);
            pass.set_bind_group(0, &pre_bgs[idx], &[]);
            pass.draw(0..3, 0..1);
        }
    }
    // BRDF LUT: a single environment-independent integration pass (no bindings).
    {
        let attachments = [RenderPassColorAttachment {
            view: &brdf_view,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK),
                store: StoreOp::Store,
            },
            base_array_layer: 0,
            base_mip_level: 0,
        }];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("IBL BRDF LUT"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&brdf_pipeline);
        pass.draw(0..3, 0..1);
    }
    match encoder.finish() {
        Some(cb) => device.submit_command_buffer(cb),
        None => log::error!("IBL: filter bake encoder finish returned None; skipping submit"),
    }

    let bindings = IblGpuBindings {
        env_cube: env.cube_view,
        irradiance_cube: irradiance.cube_view,
        prefiltered_cube: prefilter_view,
        brdf_lut: brdf_view,
        sampler,
    };

    let mut keep_views = vec![
        env.cube_view,
        irradiance.cube_view,
        prefilter_view,
        brdf_view,
    ];
    keep_views.extend(env.face_views);
    keep_views.extend(irradiance.face_views);
    let mut keep_textures = vec![
        env.texture,
        irradiance.texture,
        prefilter_texture,
        brdf_texture,
    ];
    // The equirect source is only read during the bake, but it must outlive the
    // submission that samples it.
    if let Some((texture, view)) = equirect_keep {
        keep_textures.push(texture);
        keep_views.push(view);
    }
    let mut keep_buffers = sky_bufs;
    keep_buffers.extend(irr_bufs);
    keep_buffers.extend(pre_bufs);

    Ok(IblResources {
        bindings,
        _keep_textures: keep_textures,
        _keep_views: keep_views,
        _keep_buffers: keep_buffers,
    })
}

/// A uniform-buffer bind-group entry.
fn uniform_bg_entry<'a>(binding: u32, buffer: BufferId) -> BindGroupEntry<'a> {
    BindGroupEntry {
        binding,
        resource: BindingResource::Buffer(BufferBinding {
            buffer,
            offset: 0,
            size: None,
        }),
        _phantom: std::marker::PhantomData,
    }
}

/// Records one fullscreen-triangle pass per cube face into `face_views`,
/// clearing then drawing with `pipeline` + the matching per-face bind group.
fn record_face_passes(
    encoder: &mut dyn khora_core::renderer::traits::CommandEncoder,
    pipeline: &khora_core::renderer::api::pipeline::RenderPipelineId,
    face_views: &[TextureViewId],
    bind_groups: &[khora_core::renderer::api::command::BindGroupId],
    label: &'static str,
) {
    for face in 0..6usize {
        let attachments = [RenderPassColorAttachment {
            view: &face_views[face],
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK),
                store: StoreOp::Store,
            },
            // The wgpu backend recreates the render target from the source
            // texture + this layer index (it ignores the view's own layer), so
            // this MUST be the real face index — else every face renders into
            // layer 0 and the other five stay black.
            base_array_layer: face as u32,
            base_mip_level: 0,
        }];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_groups[face], &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A bind group of (source texture @0, sampler @1, per-face basis uniform @2).
///
/// Shared by every bake pass that reads a texture per face: the irradiance
/// convolution and the specular prefilter (which bind the env **cube**), and
/// the equirectangular projection (which binds a **2D** lat-long map). Only the
/// layout's declared view dimension differs; the entry shape is identical.
fn sampled_texture_bind_group(
    device: &dyn GraphicsDevice,
    layout: khora_core::renderer::api::command::BindGroupLayoutId,
    source_view: TextureViewId,
    sampler: SamplerId,
    basis_buffer: BufferId,
) -> Result<khora_core::renderer::api::command::BindGroupId, RenderError> {
    device
        .create_bind_group(&BindGroupDescriptor {
            label: Some("IBL Texture-Sample BG"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source_view),
                    _phantom: std::marker::PhantomData,
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(sampler),
                    _phantom: std::marker::PhantomData,
                },
                uniform_bg_entry(2, basis_buffer),
            ],
        })
        .map_err(RenderError::ResourceError)
}
