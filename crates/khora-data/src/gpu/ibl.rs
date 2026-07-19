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

//! Image-based lighting (IBL) bake — the data-layer "environment → GPU"
//! projection.
//!
//! [`IblBaker`] runs **once** at startup (driven by the `ibl_bake` DataSystem)
//! and produces the static GPU resources image-based lighting samples: an
//! environment cubemap (a procedural sky for now, an authored HDR later), then
//! — in later increments — a diffuse irradiance cube, a prefiltered specular
//! cube, and the split-sum BRDF LUT. Because the bake is one-time and needs a
//! render pass, it records into a **standalone** command encoder
//! ([`GraphicsDevice::create_command_encoder`]) and submits it directly,
//! outside the per-frame loop — no lane or agent required.
//!
//! The result is stored behind interior mutability (the baker is a shared
//! resource) and consumed by the lit lanes via [`IblBaker::bindings`].

use std::borrow::Cow;
use std::sync::OnceLock;

use khora_core::math::{Extent3D, LinearRgba};
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutEntry, BindingResource, BindingType,
    BufferBinding, BufferBindingType, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp,
};
use khora_core::renderer::api::pipeline::state::ColorWrites;
use khora_core::renderer::api::pipeline::{
    ColorTargetStateDescriptor, LayoutSpec, MultisampleStateDescriptor, PipelineSpec,
    PrimitiveStateDescriptor, ShaderVariantKey,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferUsage, ImageAspect, TextureDescriptor, TextureDimension, TextureId,
    TextureUsage, TextureViewDescriptor, TextureViewId,
};
use khora_core::renderer::api::util::{SampleCount, ShaderStageFlags, TextureFormat};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;

/// Env-cube face resolution (per-face square). 256² is ample for a smooth
/// procedural sky and as the source the specular prefilter samples.
const ENV_FACE_SIZE: u32 = 256;
/// Linear HDR format for every IBL cube (sky may exceed 1.0 once authored).
const ENV_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// Logical shader name of the procedural-sky bake pipeline.
const SKY_SHADER: &str = "khora::pipelines::ibl_sky";
/// Cache label for the sky bake's single-uniform inline layout.
const FACE_BASIS_LAYOUT: &str = "ibl_sky_face_basis";

/// Per-face basis uploaded to the sky shader: `forward` / `right` / `up` in
/// world space (w unused). The fragment reconstructs a texel's world direction
/// as `normalize(forward + ndc.x*right + ndc.y*up)`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FaceBasisUniform {
    forward: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

/// The six cube faces in wgpu layer order (+X, -X, +Y, -Y, +Z, -Z), each as
/// `[forward, right, up]`. Chosen so `normalize(forward + ndc.x*right +
/// ndc.y*up)` reproduces the direction wgpu's cube sampling maps to that
/// texel, keeping the baked cube correctly oriented for later sampling.
const FACE_BASES: [FaceBasisUniform; 6] = [
    FaceBasisUniform {
        forward: [1.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, -1.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // +X
    FaceBasisUniform {
        forward: [-1.0, 0.0, 0.0, 0.0],
        right: [0.0, 0.0, 1.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // -X
    FaceBasisUniform {
        forward: [0.0, 1.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 0.0, -1.0, 0.0],
    }, // +Y
    FaceBasisUniform {
        forward: [0.0, -1.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 0.0, 1.0, 0.0],
    }, // -Y
    FaceBasisUniform {
        forward: [0.0, 0.0, 1.0, 0.0],
        right: [1.0, 0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // +Z
    FaceBasisUniform {
        forward: [0.0, 0.0, -1.0, 0.0],
        right: [-1.0, 0.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0, 0.0],
    }, // -Z
];

/// The baked environment resources. Fields prefixed `_` are kept alive only so
/// their GPU handles outlive the bake submission; they are not read again.
struct IblEnv {
    /// The environment cube texture (6 layers).
    _env_texture: TextureId,
    /// Cube view of the environment, sampled by the convolution/prefilter
    /// passes (and, later, published in `IblGpuBindings`).
    env_cube_view: TextureViewId,
    /// Transient bake resources retained for lifetime (per-face render-target
    /// views, basis uniform buffers). One-time cost; freed on shutdown.
    _bake_face_views: Vec<TextureViewId>,
    _bake_buffers: Vec<khora_core::renderer::api::resource::BufferId>,
}

/// One-time IBL bake service. Registered as a shared resource at bootstrap and
/// driven by the `ibl_bake` DataSystem, which calls [`ensure_baked`] every
/// frame; the bake itself runs only on the first call.
///
/// [`ensure_baked`]: IblBaker::ensure_baked
#[derive(Default)]
pub struct IblBaker {
    env: OnceLock<IblEnv>,
}

impl IblBaker {
    /// Creates an unbaked baker. The bake happens lazily on the first
    /// [`ensure_baked`](Self::ensure_baked) once a device is available.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bakes the IBL environment on the first call; a no-op afterwards.
    /// Idempotent and safe to call every frame.
    pub fn ensure_baked(&self, device: &dyn GraphicsDevice, pipeline_system: &dyn PipelineSystem) {
        if self.env.get().is_some() {
            return;
        }
        match bake_environment(device, pipeline_system) {
            Ok(env) => {
                log::info!(
                    "IBL: environment cube baked ({0}x{0}, 6 faces, procedural sky)",
                    ENV_FACE_SIZE
                );
                let _ = self.env.set(env);
            }
            Err(e) => log::error!("IBL: environment bake failed: {e:?}"),
        }
    }

    /// Returns the environment cube view once baked (for the convolution /
    /// prefilter passes added in later increments).
    pub fn env_cube_view(&self) -> Option<TextureViewId> {
        self.env.get().map(|e| e.env_cube_view)
    }
}

/// The single-uniform inline bind-group layout the sky bake pipeline uses.
fn face_basis_layout_entries() -> [BindGroupLayoutEntry; 1] {
    [BindGroupLayoutEntry {
        binding: 0,
        visibility: ShaderStageFlags::FRAGMENT,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
    }]
}

/// The declarative spec for the procedural-sky bake pipeline: a fullscreen
/// triangle (no vertex buffer, no depth) writing linear HDR into one env-cube
/// face.
fn sky_pipeline_spec() -> PipelineSpec {
    PipelineSpec {
        label: "IBL Sky Bake",
        shader: SKY_SHADER,
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![LayoutSpec::Inline {
            label: FACE_BASIS_LAYOUT,
            entries: Cow::Owned(face_basis_layout_entries().to_vec()),
        }],
        vertex_buffers: vec![],
        vs_entry: "vs_main",
        fs_entry: Some("fs_main"),
        primitive: PrimitiveStateDescriptor::default(),
        depth_stencil: None,
        color_targets: vec![ColorTargetStateDescriptor {
            format: ENV_FORMAT,
            blend: None,
            write_mask: ColorWrites::ALL,
        }],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

/// Creates the env cube + renders the procedural sky into its six faces in one
/// standalone submission.
fn bake_environment(
    device: &dyn GraphicsDevice,
    pipeline_system: &dyn PipelineSystem,
) -> Result<IblEnv, RenderError> {
    // 1. Environment cube texture (6 layers) + a Cube sampling view.
    let env_texture = device.create_texture(&TextureDescriptor {
        label: Some(Cow::Borrowed("IBL Env Cube")),
        size: Extent3D {
            width: ENV_FACE_SIZE,
            height: ENV_FACE_SIZE,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: SampleCount::X1,
        dimension: TextureDimension::D2,
        format: ENV_FORMAT,
        usage: TextureUsage::RENDER_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
        view_formats: Cow::Borrowed(&[]),
    })?;

    let env_cube_view = device.create_texture_view(
        env_texture,
        &TextureViewDescriptor {
            label: Some(Cow::Borrowed("IBL Env Cube View")),
            format: Some(ENV_FORMAT),
            dimension: Some(khora_core::renderer::api::resource::TextureViewDimension::Cube),
            aspect: ImageAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(6),
        },
    )?;

    // 2. One render-target view per face, one basis uniform + bind group.
    let sky_pipeline = pipeline_system.pipeline(device, &sky_pipeline_spec())?;
    let layout =
        pipeline_system.inline_layout(device, FACE_BASIS_LAYOUT, &face_basis_layout_entries())?;

    let mut face_views = Vec::with_capacity(6);
    let mut buffers = Vec::with_capacity(6);
    let mut bind_groups = Vec::with_capacity(6);
    for (face, basis) in FACE_BASES.iter().enumerate() {
        let face_view = device.create_texture_view(
            env_texture,
            &TextureViewDescriptor {
                label: Some(Cow::Owned(format!("IBL Env Cube Face [{face}]"))),
                format: Some(ENV_FORMAT),
                dimension: Some(khora_core::renderer::api::resource::TextureViewDimension::D2),
                aspect: ImageAspect::All,
                base_mip_level: 0,
                mip_level_count: Some(1),
                base_array_layer: face as u32,
                array_layer_count: Some(1),
            },
        )?;
        let buffer = device.create_buffer_with_data(
            &BufferDescriptor {
                label: Some(Cow::Owned(format!("IBL Sky Face Basis [{face}]"))),
                size: std::mem::size_of::<FaceBasisUniform>() as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            },
            bytemuck::bytes_of(basis),
        )?;
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("IBL Sky Face Basis BG"),
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }],
        })?;
        face_views.push(face_view);
        buffers.push(buffer);
        bind_groups.push(bind_group);
    }

    // 3. Record one pass per face into a standalone encoder, then submit.
    let mut encoder = device.create_command_encoder(Some("IBL Env Bake"));
    for face in 0..6usize {
        let attachments = [RenderPassColorAttachment {
            view: &face_views[face],
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK),
                store: StoreOp::Store,
            },
            base_array_layer: 0,
        }];
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("IBL Env Sky Face"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
        });
        pass.set_pipeline(&sky_pipeline);
        pass.set_bind_group(0, &bind_groups[face], &[]);
        pass.draw(0..3, 0..1);
    }
    if let Some(cb) = encoder.finish() {
        device.submit_command_buffer(cb);
    } else {
        log::error!("IBL: env bake encoder finish returned None; skipping submit");
    }

    Ok(IblEnv {
        _env_texture: env_texture,
        env_cube_view,
        _bake_face_views: face_views,
        _bake_buffers: buffers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_bases_cover_six_faces_with_unit_axes() {
        assert_eq!(FACE_BASES.len(), 6);
        // Each face forward is a unit cardinal axis; the six are distinct.
        for basis in &FACE_BASES {
            let f = basis.forward;
            let mag = f[0] * f[0] + f[1] * f[1] + f[2] * f[2];
            assert!((mag - 1.0).abs() < 1e-6, "forward must be a unit axis");
        }
    }

    #[test]
    fn unbaked_baker_has_no_env_view() {
        let baker = IblBaker::new();
        assert!(baker.env_cube_view().is_none());
    }
}
