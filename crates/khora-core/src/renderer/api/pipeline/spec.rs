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

//! Declarative pipeline + layout specs for the [`PipelineSystem`] backend.
//!
//! A lane describes *what* pipeline it needs ([`PipelineSpec`]) instead of
//! hand-rolling bind-group layouts + a `RenderPipelineDescriptor`. The backend
//! (`khora-infra`) compiles the shader (for the requested [`ShaderVariantKey`]),
//! resolves the bind-group layouts, builds + caches the pipeline, and returns a
//! [`RenderPipelineId`](super::RenderPipelineId).
//!
//! Caching is keyed by [`PipelineKey`] = `(shader, variant, color_format)`:
//! for a given shader + variant + target format the rest of the pipeline config
//! is fixed, so the rest of the spec need not be hashable.

use std::borrow::Cow;

use crate::renderer::api::command::{BindGroupLayoutEntry, BindingType, BufferBindingType};
use crate::renderer::api::pipeline::enums::CullMode;
use crate::renderer::api::pipeline::{
    ColorTargetStateDescriptor, DepthStencilStateDescriptor, MultisampleStateDescriptor,
    PrimitiveStateDescriptor, VertexBufferLayoutDescriptor,
};
use crate::renderer::api::util::{ShaderStageFlags, TextureFormat};

/// A scalar shader-def value used to specialize a shader variant. Floats are
/// excluded (naga_oil `#define` is integer/bool only; float consts stay
/// textually injected by the backend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShaderDefScalar {
    /// A boolean `#ifdef`-style flag.
    Bool(bool),
    /// A signed integer def.
    Int(i32),
    /// An unsigned integer def.
    UInt(u32),
}

/// Identifies a shader specialization: a sorted, deduped set of
/// `(name, value)` defs layered on top of the global `ShaderDefs`.
///
/// `EMPTY` (no defs) is the default variant. The key is `Hash`/`Eq` so it
/// participates in module + pipeline cache keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ShaderVariantKey(Vec<(&'static str, ShaderDefScalar)>);

impl ShaderVariantKey {
    /// The empty variant (no specialization defs).
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Adds (or overwrites) a boolean flag def, keeping the set sorted by name.
    pub fn flag(self, name: &'static str) -> Self {
        self.with(name, ShaderDefScalar::Bool(true))
    }

    /// Adds (or overwrites) a def, keeping the set sorted/deduped by name.
    pub fn with(mut self, name: &'static str, value: ShaderDefScalar) -> Self {
        match self.0.binary_search_by_key(&name, |(n, _)| n) {
            Ok(i) => self.0[i].1 = value,
            Err(i) => self.0.insert(i, (name, value)),
        }
        self
    }

    /// Whether this variant has no defs.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the named flag is present and set to `Bool(true)`. Used by
    /// variant-dependent layouts (e.g. the material group-2 layout, which
    /// includes only the texture bindings whose `HAS_*` flag is set).
    pub fn has_flag(&self, name: &str) -> bool {
        self.0
            .binary_search_by_key(&name, |(n, _)| n)
            .map(|i| matches!(self.0[i].1, ShaderDefScalar::Bool(true)))
            .unwrap_or(false)
    }

    /// The `(name, value)` defs, sorted by name.
    pub fn defs(&self) -> &[(&'static str, ShaderDefScalar)] {
        &self.0
    }
}

/// A canonical, engine-shared bind-group layout, owned once by the backend's
/// layout cache and referenced by every lane that needs it. The lit pipelines'
/// 4-group budget maps to `Camera`(0) / `Model`(1) / `Material`(2) /
/// `Lighting`(3); `LightingBuffer` is the 1-binding layout the lighting ring
/// buffer is created against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutKey {
    /// Group 0 — camera uniforms (view-projection, position).
    Camera,
    /// Group 1 — per-draw model matrices (dynamic offset).
    Model,
    /// Group 2 — material uniforms + PBR textures + sampler.
    Material,
    /// Group 3 — lighting uniforms + shadow atlases + sampler.
    Lighting,
    /// 1-binding layout (lighting uniform only) for the lighting ring buffer.
    LightingBuffer,
}

impl LayoutKey {
    /// Returns the bind-group layout entries for this canonical layout, for the
    /// given shader variant. Only `Material` varies by variant — it contains
    /// the uniform + sampler plus only the texture bindings whose `HAS_*` flag
    /// is set; the others ignore the variant.
    pub fn entries(&self, variant: &ShaderVariantKey) -> Vec<BindGroupLayoutEntry> {
        match self {
            LayoutKey::Camera => vec![uniform_entry(
                0,
                ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                false,
            )],
            LayoutKey::Model => {
                vec![uniform_entry(0, ShaderStageFlags::VERTEX, true)]
            }
            LayoutKey::Material => {
                crate::renderer::api::material::material_layout_entries_for_variant(variant)
            }
            LayoutKey::Lighting => {
                let mut entries = vec![uniform_entry(
                    crate::renderer::api::shadow::bindings::binding::LIGHTING_UNIFORMS,
                    ShaderStageFlags::FRAGMENT,
                    false,
                )];
                entries.extend(crate::renderer::api::shadow::shadow_bind_group_layout_entries());
                // IBL follows shadow in group 3: lighting uniform at 0, shadow
                // at 1/2/3, IBL at 4..8. (Forward+ hand-rolls its own layout and
                // places IBL at 8..12 instead — see `forward_plus_lane`.)
                entries.extend(crate::renderer::api::ibl::ibl_bind_group_layout_entries(4));
                entries
            }
            LayoutKey::LightingBuffer => {
                vec![uniform_entry(0, ShaderStageFlags::FRAGMENT, false)]
            }
        }
    }
}

/// Helper: a single uniform-buffer bind-group layout entry.
fn uniform_entry(
    binding: u32,
    visibility: ShaderStageFlags,
    has_dynamic_offset: bool,
) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset,
            min_binding_size: None,
        },
    }
}

/// How a pipeline references one bind-group layout: either a cached canonical
/// [`LayoutKey`], or a bespoke inline layout (cached by its `label`).
#[derive(Debug, Clone)]
pub enum LayoutSpec {
    /// A canonical engine layout, resolved through the backend cache.
    Named(LayoutKey),
    /// A bespoke layout (e.g. gizmo, grid, UI). Cached by `label` — labels must
    /// be unique + stable per distinct layout.
    Inline {
        /// Stable unique label = the inline layout's cache identity.
        label: &'static str,
        /// The layout's entries (used on cache miss).
        entries: Cow<'static, [BindGroupLayoutEntry]>,
    },
}

/// Cache identity for a resolved bind-group layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayoutCacheKey {
    /// A canonical layout for a given variant.
    Named(LayoutKey, ShaderVariantKey),
    /// A bespoke inline layout, identified by its label.
    Inline(&'static str),
}

impl LayoutSpec {
    /// The cache key for this layout spec under `variant`.
    pub fn cache_key(&self, variant: &ShaderVariantKey) -> LayoutCacheKey {
        match self {
            LayoutSpec::Named(key) => LayoutCacheKey::Named(*key, variant.clone()),
            LayoutSpec::Inline { label, .. } => LayoutCacheKey::Inline(label),
        }
    }
}

/// Declarative description of a render pipeline. The lane fills this each call;
/// the backend dedups via [`PipelineKey`] so repeated calls are a cache hit.
///
/// Owns its data (`'static`) so it is cheap to build and pass by reference.
#[derive(Debug, Clone)]
pub struct PipelineSpec {
    /// Debug label.
    pub label: &'static str,
    /// Logical shader module name (e.g. `"khora::pipelines::standard_pbr"`).
    pub shader: &'static str,
    /// Shader specialization (texture-set / feature flags). `empty()` default.
    pub variant: ShaderVariantKey,
    /// Bind-group layouts, in group order (0..N).
    pub bind_group_layouts: Vec<LayoutSpec>,
    /// Vertex buffer layout(s).
    pub vertex_buffers: Vec<VertexBufferLayoutDescriptor<'static>>,
    /// Vertex shader entry point.
    pub vs_entry: &'static str,
    /// Fragment shader entry point (if any).
    pub fs_entry: Option<&'static str>,
    /// Primitive / rasterization state.
    pub primitive: PrimitiveStateDescriptor,
    /// Depth-stencil state (None disables depth).
    pub depth_stencil: Option<DepthStencilStateDescriptor>,
    /// Color target(s); the format identifies the pipeline in the cache key.
    pub color_targets: Vec<ColorTargetStateDescriptor>,
    /// Multisample state.
    pub multisample: MultisampleStateDescriptor,
}

impl PipelineSpec {
    /// The cache key: `(shader, variant, first color-target format, cull_mode)`.
    /// The cull mode is part of the key because it is a rasterizer state baked
    /// into the pipeline: double-sided vs single-sided materials share a shader,
    /// variant, and format but need distinct pipelines. Everything else is fixed
    /// for a given key.
    pub fn key(&self) -> PipelineKey {
        PipelineKey {
            shader: self.shader,
            variant: self.variant.clone(),
            color_format: self.color_targets.first().map(|c| c.format),
            cull_mode: self.primitive.cull_mode,
            blend: self
                .color_targets
                .first()
                .is_some_and(|c| c.blend.is_some()),
        }
    }
}

/// Cache identity for a render pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PipelineKey {
    /// Logical shader name.
    pub shader: &'static str,
    /// Shader variant.
    pub variant: ShaderVariantKey,
    /// First color-target format (None = no color target).
    pub color_format: Option<TextureFormat>,
    /// Face-culling mode (double-sided materials render with `None`).
    pub cull_mode: Option<CullMode>,
    /// Whether the first color target blends.
    ///
    /// A single flag covers the whole transparent state: the lit lanes derive
    /// both the blend state and `depth_write_enabled` from the material's
    /// `AlphaMode::Blend`, so two pipelines can never differ in one without
    /// differing in the other.
    pub blend: bool,
}

/// Declarative description of a compute pipeline. Mirrors [`PipelineSpec`] for
/// the compute path (no vertex / depth / color state). The backend resolves the
/// bind-group layouts, compiles the compute shader for the variant, and builds +
/// caches the pipeline keyed by [`ComputePipelineKey`]. Used by Forward+ light
/// culling.
#[derive(Debug, Clone)]
pub struct ComputePipelineSpec {
    /// Debug label.
    pub label: &'static str,
    /// Logical shader module name (e.g. `"khora::pipelines::light_culling"`).
    pub shader: &'static str,
    /// Shader specialization. `empty()` default.
    pub variant: ShaderVariantKey,
    /// Bind-group layouts, in group order (0..N).
    pub bind_group_layouts: Vec<LayoutSpec>,
    /// Compute shader entry point.
    pub entry_point: &'static str,
}

impl ComputePipelineSpec {
    /// The cache key: `(shader, variant)`. For a given shader + variant the
    /// rest of the compute config is fixed.
    pub fn key(&self) -> ComputePipelineKey {
        ComputePipelineKey {
            shader: self.shader,
            variant: self.variant.clone(),
        }
    }
}

/// Cache identity for a compute pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComputePipelineKey {
    /// Logical shader name.
    pub shader: &'static str,
    /// Shader variant.
    pub variant: ShaderVariantKey,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::api::pipeline::enums::CullMode;

    #[test]
    fn pipeline_key_distinguishes_cull_mode() {
        // Single-sided vs double-sided materials share shader + variant +
        // format but must resolve to distinct cached pipelines.
        let single_sided = PipelineKey {
            shader: "khora::pipelines::standard_pbr",
            variant: ShaderVariantKey::empty(),
            color_format: None,
            cull_mode: Some(CullMode::Back),
            blend: false,
        };
        let double_sided = PipelineKey {
            cull_mode: None,
            ..single_sided.clone()
        };
        assert_ne!(single_sided, double_sided);
    }

    #[test]
    fn pipeline_key_distinguishes_blend() {
        // An opaque and an alpha-blended material share shader + variant +
        // format + cull mode, but their pipelines differ in blend state and
        // depth-write, so they must not collide in the cache.
        let opaque = PipelineKey {
            shader: "khora::pipelines::standard_pbr",
            variant: ShaderVariantKey::empty(),
            color_format: None,
            cull_mode: Some(CullMode::Back),
            blend: false,
        };
        let blended = PipelineKey {
            blend: true,
            ..opaque.clone()
        };
        assert_ne!(opaque, blended);
    }
}
