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

//! What the shadows lane builds on the device for one level, and the frame's
//! shadow rendering.
//!
//! [`ShadowGpu`] is built whole by the lane's `prepare` — the depth pipeline,
//! the two uniform rings, the two atlases at the level's resolution, the
//! comparison sampler — and given back whole by its `shutdown`, or when a
//! level of another resolution replaces it. The lane holds it by value and
//! records through `&mut self`: no lock.

use crate::render_lane::util::DynamicUniformRingBuffer;
use khora_core::renderer::api::gpu_scene::{GpuMesh, ModelUniforms};
use khora_core::renderer::api::pipeline::RenderPipelineId;
use khora_core::renderer::api::resource::{CameraUniformData, SamplerId};
use khora_core::renderer::api::shadow::{ShadowEntries, ShadowEntry, ShadowGpuBindings};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::{traits::CommandEncoder, GraphicsDevice};
use khora_data::assets::Assets;
use khora_data::render::RenderWorld;

use super::atlas_2d::Atlas2D;
use super::atlas_cube::AtlasCube;
use super::pass;

/// The resolutions and capacities of one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tier {
    /// Side of each 2D atlas layer, in texels.
    pub atlas_2d_resolution: u32,
    /// Directional and spot casters a frame shadows.
    pub atlas_2d_max_lights: u32,
    /// Side of each cube face, in texels.
    pub cube_face_resolution: u32,
    /// Point casters a frame shadows.
    pub cube_max_lights: u32,
}

impl Tier {
    /// The graphics memory the level's atlases hold: four bytes a texel.
    pub const fn atlas_bytes(&self) -> u64 {
        let side_2d = self.atlas_2d_resolution as u64;
        let side_cube = self.cube_face_resolution as u64;
        side_2d * side_2d * self.atlas_2d_max_lights as u64 * 4
            + side_cube * side_cube * self.cube_max_lights as u64 * 6 * 4
    }
}

/// Everything a level builds on the device.
pub struct ShadowGpu {
    /// The level it was built for.
    pub tier: Tier,
    pipeline: RenderPipelineId,
    camera_ring: DynamicUniformRingBuffer,
    model_ring: DynamicUniformRingBuffer,
    atlas_2d: Atlas2D,
    atlas_cube: AtlasCube,
    sampler: SamplerId,
}

impl ShadowGpu {
    /// Builds a level's resources. A refusal halfway gives back what was
    /// made before it, so a retried prepare leaks nothing.
    pub fn build(
        device: &dyn GraphicsDevice,
        pipelines: &dyn PipelineSystem,
        tier: Tier,
        label: &str,
    ) -> Result<Self, RenderError> {
        use crate::render_lane::util::dynamic_uniform_ring_buffer::{
            DEFAULT_MAX_ELEMENTS, MIN_UNIFORM_ALIGNMENT,
        };

        // Layouts and the depth-only pipeline belong to the PipelineSystem,
        // which caches them: nothing to give back.
        let camera_layout = pipelines.inline_layout(
            device,
            SHADOW_CAMERA_LAYOUT_LABEL,
            &shadow_camera_layout_entries(),
        )?;
        let model_layout = pipelines.inline_layout(
            device,
            SHADOW_MODEL_LAYOUT_LABEL,
            &shadow_model_layout_entries(),
        )?;
        let pipeline = pipelines.pipeline(device, &shadow_pipeline_spec())?;

        let camera_ring = DynamicUniformRingBuffer::new(
            device,
            camera_layout,
            0,
            std::mem::size_of::<CameraUniformData>() as u32,
            tier.atlas_2d_max_lights + tier.cube_max_lights * 6,
            MIN_UNIFORM_ALIGNMENT,
            "Shadow Camera Ring",
        )
        .map_err(RenderError::ResourceError)?;
        let model_ring = match DynamicUniformRingBuffer::new(
            device,
            model_layout,
            0,
            std::mem::size_of::<ModelUniforms>() as u32,
            DEFAULT_MAX_ELEMENTS,
            MIN_UNIFORM_ALIGNMENT,
            "Shadow Model Ring",
        ) {
            Ok(ring) => ring,
            Err(error) => {
                camera_ring.destroy(device);
                return Err(RenderError::ResourceError(error));
            }
        };
        let give_back_rings = |device: &dyn GraphicsDevice| {
            camera_ring.destroy(device);
            model_ring.destroy(device);
        };

        let atlas_2d = match Atlas2D::create(
            device,
            tier.atlas_2d_resolution,
            tier.atlas_2d_max_lights,
            &format!("{label} Atlas 2D"),
        ) {
            Ok(atlas) => atlas,
            Err(error) => {
                give_back_rings(device);
                return Err(error);
            }
        };
        let atlas_cube = match AtlasCube::create(
            device,
            tier.cube_face_resolution,
            tier.cube_max_lights,
            &format!("{label} Atlas Cube"),
        ) {
            Ok(atlas) => atlas,
            Err(error) => {
                atlas_2d.destroy(device);
                give_back_rings(device);
                return Err(error);
            }
        };
        let sampler = match device.create_sampler(&shadow_sampler_descriptor()) {
            Ok(sampler) => sampler,
            Err(error) => {
                atlas_cube.destroy(device);
                atlas_2d.destroy(device);
                give_back_rings(device);
                return Err(RenderError::ResourceError(error));
            }
        };

        log::info!(
            "{label}: atlases built — 2D({side}×{side}, {} layers), Cube({face}×{face}, {} cubes)",
            tier.atlas_2d_max_lights,
            tier.cube_max_lights,
            side = tier.atlas_2d_resolution,
            face = tier.cube_face_resolution,
        );
        Ok(Self {
            tier,
            pipeline,
            camera_ring,
            model_ring,
            atlas_2d,
            atlas_cube,
            sampler,
        })
    }

    /// The bindings the lit lanes sample the atlases through.
    pub fn bindings(&self) -> ShadowGpuBindings {
        super::bindings::build_bindings(&self.atlas_2d, &self.atlas_cube, self.sampler)
    }

    /// Renders one frame's shadows into `encoder` and fills `entries` with
    /// each shadowed light's: the frame's 2D casters up to the level's
    /// capacity, then its point casters up to its own.
    #[allow(clippy::too_many_arguments)] // one parameter per render input; a config struct would obscure the call sites
    pub fn render(
        &mut self,
        render_world: &RenderWorld,
        shadow_view: &khora_data::flow::ShadowView,
        device: &dyn GraphicsDevice,
        encoder: &mut dyn CommandEncoder,
        gpu_meshes: &Assets<GpuMesh>,
        entries: &mut ShadowEntries,
        label: &str,
    ) {
        use khora_core::math::Mat4;
        use khora_data::flow::ShadowMatrices;

        let tier = self.tier;
        self.camera_ring.advance();
        self.model_ring.advance();
        entries.0.clear();

        let mut next_atlas_2d_index: u32 = 0;
        let mut next_cube_layer: u32 = 0;
        let mut passes: Vec<pass::AttachmentPass> = Vec::new();

        for (i, light) in render_world.lights.iter().enumerate() {
            let Some(matrices) = shadow_view.matrices.get(&i) else {
                continue;
            };
            match matrices {
                ShadowMatrices::Single(view_proj) => {
                    if next_atlas_2d_index >= tier.atlas_2d_max_lights {
                        log::warn!(
                            "{label}: 2D atlas full ({next_atlas_2d_index} / {}), dropping shadow for light {i}",
                            tier.atlas_2d_max_lights
                        );
                        continue;
                    }
                    let layer = next_atlas_2d_index;
                    next_atlas_2d_index += 1;
                    entries.insert(
                        i,
                        ShadowEntry::Atlas2D {
                            view_proj: *view_proj,
                            atlas_index: layer as i32,
                        },
                    );
                    if let Some(p) = super::atlas_2d::collect_pass(
                        self.atlas_2d.view,
                        layer,
                        light.position,
                        *view_proj,
                        device,
                        render_world,
                        gpu_meshes,
                        &mut self.camera_ring,
                        &mut self.model_ring,
                    ) {
                        passes.push(p);
                    }
                }
                ShadowMatrices::Cube(face_view_projs) => {
                    if next_cube_layer >= tier.cube_max_lights {
                        log::warn!(
                            "{label}: cube atlas full ({next_cube_layer} / {}), dropping shadow for point light {i}",
                            tier.cube_max_lights
                        );
                        continue;
                    }
                    let far_plane = match &light.light_type {
                        khora_core::renderer::light::LightType::Point(p) => p.range,
                        _ => continue,
                    };
                    let cube_layer = next_cube_layer;
                    next_cube_layer += 1;
                    let face_view_projs: [Mat4; 6] = **face_view_projs;
                    entries.insert(
                        i,
                        ShadowEntry::Cube {
                            face_view_projs: Box::new(face_view_projs),
                            cube_array_index: cube_layer as i32,
                            light_pos: light.position,
                            far_plane,
                        },
                    );
                    passes.extend(super::atlas_cube::collect_passes(
                        &self.atlas_cube.face_views,
                        cube_layer,
                        light.position,
                        &face_view_projs,
                        device,
                        render_world,
                        gpu_meshes,
                        &mut self.camera_ring,
                        &mut self.model_ring,
                    ));
                }
            }
        }

        for attachment in &passes {
            pass::record_depth_pass(encoder, &self.pipeline, attachment);
        }
    }

    /// Gives every resource back to the device. The pipeline and the
    /// layouts are the `PipelineSystem`'s, so they stay.
    pub fn destroy(self, device: &dyn GraphicsDevice) {
        self.camera_ring.destroy(device);
        self.model_ring.destroy(device);
        self.atlas_2d.destroy(device);
        self.atlas_cube.destroy(device);
        if let Err(e) = device.destroy_sampler(self.sampler) {
            log::warn!("ShadowGpu: failed to destroy sampler: {:?}", e);
        }
    }
}

/// The comparison sampler both atlases are sampled through.
fn shadow_sampler_descriptor() -> khora_core::renderer::api::resource::SamplerDescriptor<'static> {
    use khora_core::renderer::api::pipeline::enums::CompareFunction;
    use khora_core::renderer::api::resource::{
        AddressMode, FilterMode, MipmapFilterMode, SamplerDescriptor,
    };
    use std::borrow::Cow;

    SamplerDescriptor {
        label: Some(Cow::Borrowed("Shadow Sampler")),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Nearest,
        lod_min_clamp: 0.0,
        lod_max_clamp: 1.0,
        compare: Some(CompareFunction::LessEqual),
        anisotropy_clamp: 1,
        border_color: None,
    }
}

// ─── Free functions (CLAD: declarative spec + bespoke layouts) ───

/// Stable cache label for the shadow camera (light view-projection) layout.
const SHADOW_CAMERA_LAYOUT_LABEL: &str = "shadow_camera_layout";
/// Stable cache label for the shadow model-uniform layout.
const SHADOW_MODEL_LAYOUT_LABEL: &str = "shadow_model_layout";

/// Bespoke shadow camera layout: a single dynamic-offset uniform buffer
/// (vertex stage only).
fn shadow_camera_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    crate::render_lane::util::single_uniform_layout(
        khora_core::renderer::api::util::ShaderStageFlags::VERTEX,
        true,
    )
}

/// Bespoke shadow model layout: a single dynamic-offset uniform buffer
/// (vertex stage only).
fn shadow_model_layout_entries() -> Vec<khora_core::renderer::api::command::BindGroupLayoutEntry> {
    shadow_camera_layout_entries()
}

/// The declarative depth-only pipeline spec for the shadow pass — built each
/// call, deduped by the `PipelineSystem`. No fragment / color state.
fn shadow_pipeline_spec() -> khora_core::renderer::api::pipeline::PipelineSpec {
    use khora_core::renderer::api::gpu_scene::Mesh;
    use khora_core::renderer::api::pipeline::enums::{CompareFunction, PrimitiveTopology};
    use khora_core::renderer::api::pipeline::state::{DepthBiasState, StencilFaceState};
    use khora_core::renderer::api::pipeline::{
        DepthStencilStateDescriptor, LayoutSpec, MultisampleStateDescriptor, PipelineSpec,
        PrimitiveStateDescriptor,
    };
    use khora_core::renderer::api::resource::{SampleCount, TextureFormat};
    use khora_core::renderer::api::shader::ShaderVariantKey;
    use std::borrow::Cow;

    PipelineSpec {
        label: "Shadow Pass Pipeline",
        shader: "khora::pipelines::shadow_pass",
        variant: ShaderVariantKey::empty(),
        bind_group_layouts: vec![
            LayoutSpec::Inline {
                label: SHADOW_CAMERA_LAYOUT_LABEL,
                entries: Cow::Owned(shadow_camera_layout_entries()),
            },
            LayoutSpec::Inline {
                label: SHADOW_MODEL_LAYOUT_LABEL,
                entries: Cow::Owned(shadow_model_layout_entries()),
            },
        ],
        vertex_buffers: vec![Mesh::vertex_buffer_layout()],
        vs_entry: "vs_main",
        fs_entry: None,
        primitive: PrimitiveStateDescriptor {
            topology: PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilStateDescriptor {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: CompareFunction::Less,
            stencil_front: StencilFaceState::default(),
            stencil_back: StencilFaceState::default(),
            stencil_read_mask: 0,
            stencil_write_mask: 0,
            bias: DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            },
        }),
        color_targets: vec![],
        multisample: MultisampleStateDescriptor {
            count: SampleCount::X1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_pass_pipeline_binds_the_mesh_vertex() {
        crate::tests::assert_binds_the_mesh_vertex(&shadow_pipeline_spec());
    }
}
