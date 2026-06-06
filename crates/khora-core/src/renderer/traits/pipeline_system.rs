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

//! The shader/pipeline backend contract.
//!
//! `PipelineSystem` is a **backend** in the SAA sense — abstract trait here in
//! `khora-core`, concrete impl in `khora-infra` (naga_oil + the wgpu device),
//! injected via `runtime.backends`, consumed via the trait by render lanes and
//! the material projection. Exactly the pattern of [`GraphicsDevice`] /
//! [`RenderSystem`](super::RenderSystem): it keeps the lanes pure consumers —
//! they no longer hand-roll bind-group layouts or pipelines, nor depend on a
//! shader compiler.
//!
//! It centralizes:
//! - **Layout cache** — canonical [`LayoutKey`] layouts (Camera/Model/Material/
//!   Lighting) created once and shared; bespoke layouts cached by label.
//! - **Pipeline cache** — keyed by `(shader, variant, color_format)`, deduped.
//! - **Shader variants** — specialization defs (`#ifdef`) keyed by
//!   [`ShaderVariantKey`].
//! - **Hot-reload** — overlay sources + recompose (Phase 3).

use crate::renderer::api::command::{BindGroupLayoutEntry, BindGroupLayoutId};
use crate::renderer::api::pipeline::{
    ComputePipelineId, ComputePipelineSpec, LayoutKey, PipelineSpec, RenderPipelineId,
    ShaderVariantKey,
};
use crate::renderer::error::RenderError;
use crate::renderer::traits::GraphicsDevice;

/// Backend that compiles shaders and builds/caches bind-group layouts and
/// render pipelines on demand. See the module docs.
pub trait PipelineSystem: Send + Sync {
    /// Returns the canonical bind-group layout for `key` under `variant`,
    /// creating + caching it on first request. Both lit lanes (pipeline slot)
    /// and the material projection (group-2 bind group) obtain layouts here, so
    /// they share the same id.
    fn layout(
        &self,
        device: &dyn GraphicsDevice,
        key: LayoutKey,
        variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError>;

    /// Returns a bespoke (inline) bind-group layout, identified + cached by its
    /// stable `label`, creating it on first request. Lanes with non-canonical
    /// layouts (shadow / UI / overlay / unlit) obtain their layout ids here so
    /// they can build ring buffers / bind groups against the same id the
    /// pipeline was built with.
    fn inline_layout(
        &self,
        device: &dyn GraphicsDevice,
        label: &'static str,
        entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError>;

    /// Returns the render pipeline for `spec`, creating + caching it on first
    /// request (keyed by `(shader, variant, color_format)`). Resolves the
    /// spec's bind-group layouts, compiles the shader for the variant, and
    /// builds the pipeline. Lanes call this every frame; it is a cache hit
    /// after the first.
    fn pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError>;

    /// Returns the compute pipeline for `spec`, creating + caching it on first
    /// request (keyed by `(shader, variant)`). Resolves the spec's bind-group
    /// layouts, compiles the shader for the variant, and builds the pipeline.
    /// Used by Forward+ light culling.
    fn compute_pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError>;

    /// Hot-reload: override a logical shader source (lib or pipeline) with new
    /// text, marking it (and dependent pipelines) for recompose. Default no-op
    /// until the hot-reload phase wires it.
    fn set_overlay_source(&self, _logical_path: &str, _source: String) {}

    /// Hot-reload: recompose any modules marked dirty by
    /// [`set_overlay_source`](Self::set_overlay_source) and rebuild the
    /// affected cached pipelines in place. Default no-op.
    fn recompose_dirty(&self, _device: &dyn GraphicsDevice) -> Result<(), RenderError> {
        Ok(())
    }
}
