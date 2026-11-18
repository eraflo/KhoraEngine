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

//! The shader and pipeline backend contract: compiles shaders, caches
//! bind-group layouts and pipelines, and takes hot-reload overlays.

use crate::renderer::api::command::{BindGroupLayoutEntry, BindGroupLayoutId};
use crate::renderer::api::pipeline::{
    ComputePipelineId, ComputePipelineSpec, LayoutKey, PipelineSpec, RenderPipelineId,
};
use crate::renderer::api::shader::ShaderVariantKey;
use crate::renderer::error::RenderError;
use crate::renderer::traits::GraphicsDevice;

/// Backend that compiles shaders and caches bind-group layouts and pipelines.
///
/// Lanes resolve their pipelines here by name, so they never hold a shader
/// compiler. The implementation lives in `khora-infra`.
///
/// # Called from many threads at once
///
/// Lanes build their pipelines in `prepare`, inside concurrent waves. An
/// implementation may hold its own lock while it calls the
/// [`GraphicsDevice`], whose locks are leaves below it; it must never be
/// called back by the device, nor hold its lock while waiting on anything
/// else.
pub trait PipelineSystem: Send + Sync {
    /// Returns the canonical bind-group layout for `key` under `variant`.
    ///
    /// Created on first request, then cached: lit lanes and the material
    /// projection get the same id for the same key.
    ///
    /// # Errors
    ///
    /// Returns an error if the device fails to create the layout.
    fn layout(
        &self,
        device: &dyn GraphicsDevice,
        key: LayoutKey,
        variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError>;

    /// Returns a bespoke bind-group layout, cached by its stable `label`.
    ///
    /// A lane with a non-canonical layout gets here the same id its pipeline
    /// was built with, so its bind groups match it.
    ///
    /// # Errors
    ///
    /// Returns an error if the device fails to create the layout.
    fn inline_layout(
        &self,
        device: &dyn GraphicsDevice,
        label: &'static str,
        entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError>;

    /// Returns the render pipeline for `spec`, cached by shader, variant and
    /// colour format.
    ///
    /// Cheap after the first call, so lanes may call it every frame.
    ///
    /// # Errors
    ///
    /// Returns an error if the shader fails to compose or compile, or the
    /// device fails to build the pipeline.
    fn pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError>;

    /// Returns the compute pipeline for `spec`, cached by shader and variant.
    ///
    /// # Errors
    ///
    /// Returns an error if the shader fails to compose or compile, or the
    /// device fails to build the pipeline.
    fn compute_pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError>;

    /// Returns whether this system composes the module `logical_path` names: a
    /// lib or a pipeline it registers.
    ///
    /// The hot reload hands over overlays of these modules only. The default
    /// composes every name.
    fn composes(&self, _logical_path: &str) -> bool {
        true
    }

    /// Overrides a logical shader source with new text for hot reload.
    ///
    /// Marks it and its dependent pipelines for the next
    /// [`recompose_dirty`](Self::recompose_dirty). The default does nothing.
    fn set_overlay_source(&self, _logical_path: &str, _source: String) {}

    /// Drops every overlay set by [`set_overlay_source`](Self::set_overlay_source).
    ///
    /// Each module goes back to the engine's own source at the next
    /// [`recompose_dirty`](Self::recompose_dirty). The default does nothing.
    fn clear_overlays(&self) {}

    /// Recomposes the modules marked dirty and rebuilds their cached pipelines
    /// in place.
    ///
    /// The default does nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if a module fails to compose or a pipeline to rebuild.
    fn recompose_dirty(&self, _device: &dyn GraphicsDevice) -> Result<(), RenderError> {
        Ok(())
    }
}
