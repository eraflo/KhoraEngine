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

//! `WgpuPipelineSystem` — the concrete [`PipelineSystem`] backend.
//!
//! Wraps `naga_oil` (shader composition + variant specialization) and the
//! abstract `GraphicsDevice` (layout / pipeline creation), with three caches:
//! bind-group layouts (by `LayoutCacheKey`), shader modules (by
//! `(name, variant)`), and render pipelines (by `PipelineKey`). The `.wgsl`
//! sources live next to this file (`shaders/`), embedded at compile time —
//! they were relocated here from `khora-lanes` so the shader compiler is a
//! backend, not lane code.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use khora_core::renderer::api::command::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindGroupLayoutId, ComputePipelineDescriptor,
    ComputePipelineId,
};
use khora_core::renderer::api::core::ShaderModuleId;
use khora_core::renderer::api::pipeline::{
    ComputePipelineKey, ComputePipelineSpec, LayoutCacheKey, LayoutKey, PipelineKey,
    PipelineLayoutDescriptor, PipelineSpec, RenderPipelineDescriptor, RenderPipelineId,
    ShaderVariantKey,
};
use khora_core::renderer::api::shader_defs::ShaderDefs;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{GraphicsDevice, PipelineSystem};

use dependencies::{affected_compute_pipelines, affected_render_pipelines, readd_lib_module};
use naga_oil::compose::{ComposableModuleDescriptor, Composer, ShaderDefValue, ShaderLanguage};
use rebuild::{
    inject_float_defs, layout_label, make_module, rebuild_compute_pipeline,
    rebuild_render_pipeline, resolve_layout,
};

mod dependencies;
mod rebuild;

/// (logical import path, source) for each composable lib module.
const LIB_MODULES: &[(&str, &str)] = &[
    (
        "khora::std::camera",
        include_str!("../shaders/lib/std/camera.wgsl"),
    ),
    (
        "khora::std::model",
        include_str!("../shaders/lib/std/model.wgsl"),
    ),
    (
        "khora::std::material",
        include_str!("../shaders/lib/std/material.wgsl"),
    ),
    (
        "khora::std::material_textures",
        include_str!("../shaders/lib/std/material_textures.wgsl"),
    ),
    (
        "khora::std::vertex",
        include_str!("../shaders/lib/std/vertex.wgsl"),
    ),
    (
        "khora::lighting::structs",
        include_str!("../shaders/lib/lighting/structs.wgsl"),
    ),
    (
        "khora::lighting::uniforms",
        include_str!("../shaders/lib/lighting/uniforms.wgsl"),
    ),
    (
        "khora::lighting::attenuation",
        include_str!("../shaders/lib/lighting/attenuation.wgsl"),
    ),
    (
        "khora::lighting::pbr",
        include_str!("../shaders/lib/lighting/pbr.wgsl"),
    ),
    (
        "khora::shadow::bindings",
        include_str!("../shaders/lib/shadow/bindings.wgsl"),
    ),
    (
        "khora::shadow::sample_2d",
        include_str!("../shaders/lib/shadow/sample_2d.wgsl"),
    ),
    (
        "khora::shadow::sample_cube",
        include_str!("../shaders/lib/shadow/sample_cube.wgsl"),
    ),
];

/// (logical name, source) for each composable pipeline module.
const PIPELINE_MODULES: &[(&str, &str)] = &[
    (
        "khora::pipelines::lit_forward",
        include_str!("../shaders/pipelines/lit_forward.wgsl"),
    ),
    (
        "khora::pipelines::forward_plus",
        include_str!("../shaders/pipelines/forward_plus.wgsl"),
    ),
    (
        "khora::pipelines::standard_pbr",
        include_str!("../shaders/pipelines/standard_pbr.wgsl"),
    ),
    (
        "khora::pipelines::unlit",
        include_str!("../shaders/pipelines/unlit.wgsl"),
    ),
    (
        "khora::pipelines::wireframe",
        include_str!("../shaders/pipelines/wireframe.wgsl"),
    ),
    (
        "khora::pipelines::shadow_pass",
        include_str!("../shaders/pipelines/shadow_pass.wgsl"),
    ),
    (
        "khora::pipelines::light_culling",
        include_str!("../shaders/pipelines/light_culling.wgsl"),
    ),
    (
        "khora::pipelines::ui",
        include_str!("../shaders/pipelines/ui.wgsl"),
    ),
    (
        "khora::pipelines::grid",
        include_str!("../shaders/pipelines/grid.wgsl"),
    ),
    (
        "khora::pipelines::gizmo",
        include_str!("../shaders/pipelines/gizmo.wgsl"),
    ),
    (
        "khora::pipelines::ibl_sky",
        include_str!("../shaders/pipelines/ibl_sky.wgsl"),
    ),
    (
        "khora::pipelines::ibl_irradiance",
        include_str!("../shaders/pipelines/ibl_irradiance.wgsl"),
    ),
    (
        "khora::pipelines::ibl_prefilter",
        include_str!("../shaders/pipelines/ibl_prefilter.wgsl"),
    ),
    (
        "khora::pipelines::ibl_brdf_lut",
        include_str!("../shaders/pipelines/ibl_brdf_lut.wgsl"),
    ),
    (
        "khora::pipelines::skybox",
        include_str!("../shaders/pipelines/skybox.wgsl"),
    ),
    (
        "khora::pipelines::ibl_equirect",
        include_str!("../shaders/pipelines/ibl_equirect.wgsl"),
    ),
];

/// Mutable interior state behind the system's `Mutex`.
struct Inner {
    composer: Composer,
    pipelines: HashMap<&'static str, &'static str>,
    base_defs: HashMap<String, ShaderDefValue>,
    layouts: HashMap<LayoutCacheKey, BindGroupLayoutId>,
    modules: HashMap<(String, ShaderVariantKey), ShaderModuleId>,
    pipeline_cache: HashMap<PipelineKey, RenderPipelineId>,
    compute_pipeline_cache: HashMap<ComputePipelineKey, ComputePipelineId>,
    /// Full specs of cached pipelines, retained so a hot-reload can rebuild
    /// each pipeline in place under the same key.
    pipeline_specs: HashMap<PipelineKey, PipelineSpec>,
    compute_pipeline_specs: HashMap<ComputePipelineKey, ComputePipelineSpec>,
    /// Hot-reload overrides: logical path (lib import path or pipeline name)
    /// → new source. Consulted before the embedded `include_str!` source.
    overlays: HashMap<String, String>,
    /// Logical module names whose source changed and whose dependent
    /// pipelines must recompose on the next [`PipelineSystem::recompose_dirty`].
    dirty: HashSet<String>,
}

/// The wgpu/naga_oil [`PipelineSystem`] backend. Injected via
/// `runtime.backends` as `Arc<dyn PipelineSystem>`.
pub struct WgpuPipelineSystem {
    inner: Mutex<Inner>,
}

impl WgpuPipelineSystem {
    /// Builds the system: registers every lib module with the composer (fails
    /// fast on a compose error). No graphics device needed at construction.
    pub fn new() -> Result<Self, RenderError> {
        let mut composer = Composer::default();

        let base_defs: HashMap<String, ShaderDefValue> = HashMap::from([
            (
                "MAX_DIRECTIONAL_LIGHTS".to_string(),
                ShaderDefValue::UInt(ShaderDefs::MAX_DIRECTIONAL_LIGHTS),
            ),
            (
                "MAX_POINT_LIGHTS".to_string(),
                ShaderDefValue::UInt(ShaderDefs::MAX_POINT_LIGHTS),
            ),
            (
                "MAX_SPOT_LIGHTS".to_string(),
                ShaderDefValue::UInt(ShaderDefs::MAX_SPOT_LIGHTS),
            ),
            (
                "MAX_LIGHTS_PER_TILE".to_string(),
                ShaderDefValue::UInt(ShaderDefs::MAX_LIGHTS_PER_TILE),
            ),
        ]);

        for (path, source) in LIB_MODULES {
            let patched = inject_float_defs(source);
            composer
                .add_composable_module(ComposableModuleDescriptor {
                    source: &patched,
                    file_path: path,
                    language: ShaderLanguage::Wgsl,
                    shader_defs: base_defs.clone(),
                    ..Default::default()
                })
                .map_err(|e| compose_err(path, e))?;
        }

        Ok(Self {
            inner: Mutex::new(Inner {
                composer,
                pipelines: PIPELINE_MODULES.iter().copied().collect(),
                base_defs,
                layouts: HashMap::new(),
                modules: HashMap::new(),
                pipeline_cache: HashMap::new(),
                compute_pipeline_cache: HashMap::new(),
                pipeline_specs: HashMap::new(),
                compute_pipeline_specs: HashMap::new(),
                overlays: HashMap::new(),
                dirty: HashSet::new(),
            }),
        })
    }
}

impl PipelineSystem for WgpuPipelineSystem {
    fn layout(
        &self,
        device: &dyn GraphicsDevice,
        key: LayoutKey,
        variant: &ShaderVariantKey,
    ) -> Result<BindGroupLayoutId, RenderError> {
        let mut inner = lock(&self.inner)?;
        let cache_key = LayoutCacheKey::Named(key, variant.clone());
        if let Some(id) = inner.layouts.get(&cache_key) {
            return Ok(*id);
        }
        let entries = key.entries(variant);
        let id = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some(layout_label(key)),
                entries: &entries,
            })
            .map_err(RenderError::ResourceError)?;
        inner.layouts.insert(cache_key, id);
        Ok(id)
    }

    fn inline_layout(
        &self,
        device: &dyn GraphicsDevice,
        label: &'static str,
        entries: &[BindGroupLayoutEntry],
    ) -> Result<BindGroupLayoutId, RenderError> {
        let mut inner = lock(&self.inner)?;
        let cache_key = LayoutCacheKey::Inline(label);
        if let Some(id) = inner.layouts.get(&cache_key) {
            return Ok(*id);
        }
        let id = device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
            .map_err(RenderError::ResourceError)?;
        inner.layouts.insert(cache_key, id);
        Ok(id)
    }

    fn pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &PipelineSpec,
    ) -> Result<RenderPipelineId, RenderError> {
        let mut inner = lock(&self.inner)?;
        let pkey = spec.key();
        if let Some(id) = inner.pipeline_cache.get(&pkey) {
            return Ok(*id);
        }

        // Resolve bind-group layouts in order.
        let mut layout_ids: Vec<BindGroupLayoutId> =
            Vec::with_capacity(spec.bind_group_layouts.len());
        for ls in &spec.bind_group_layouts {
            let id = resolve_layout(&mut inner, device, ls, &spec.variant)?;
            layout_ids.push(id);
        }

        // Pipeline layout.
        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some(Cow::Borrowed(spec.label)),
                bind_group_layouts: &layout_ids,
            })
            .map_err(RenderError::ResourceError)?;

        // Shader module (composed for the variant).
        let module = make_module(&mut inner, device, spec.shader, &spec.variant)?;

        let desc = RenderPipelineDescriptor {
            label: Some(Cow::Borrowed(spec.label)),
            vertex_shader_module: module,
            vertex_entry_point: Cow::Borrowed(spec.vs_entry),
            fragment_shader_module: spec.fs_entry.map(|_| module),
            fragment_entry_point: spec.fs_entry.map(Cow::Borrowed),
            vertex_buffers_layout: Cow::Owned(spec.vertex_buffers.clone()),
            layout: Some(pipeline_layout),
            primitive_state: spec.primitive,
            depth_stencil_state: spec.depth_stencil.clone(),
            color_target_states: Cow::Owned(spec.color_targets.clone()),
            multisample_state: spec.multisample,
        };
        let id = device
            .create_render_pipeline(&desc)
            .map_err(RenderError::ResourceError)?;
        inner.pipeline_cache.insert(pkey.clone(), id);
        inner.pipeline_specs.insert(pkey, spec.clone());
        Ok(id)
    }

    fn compute_pipeline(
        &self,
        device: &dyn GraphicsDevice,
        spec: &ComputePipelineSpec,
    ) -> Result<ComputePipelineId, RenderError> {
        let mut inner = lock(&self.inner)?;
        let ckey = spec.key();
        if let Some(id) = inner.compute_pipeline_cache.get(&ckey) {
            return Ok(*id);
        }

        // Resolve bind-group layouts in order.
        let mut layout_ids: Vec<BindGroupLayoutId> =
            Vec::with_capacity(spec.bind_group_layouts.len());
        for ls in &spec.bind_group_layouts {
            let id = resolve_layout(&mut inner, device, ls, &spec.variant)?;
            layout_ids.push(id);
        }

        let pipeline_layout = device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: Some(Cow::Borrowed(spec.label)),
                bind_group_layouts: &layout_ids,
            })
            .map_err(RenderError::ResourceError)?;

        let module = make_module(&mut inner, device, spec.shader, &spec.variant)?;

        let id = device
            .create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some(Cow::Borrowed(spec.label)),
                layout: Some(pipeline_layout),
                shader_module: module,
                entry_point: Cow::Borrowed(spec.entry_point),
            })
            .map_err(RenderError::ResourceError)?;
        inner.compute_pipeline_cache.insert(ckey.clone(), id);
        inner.compute_pipeline_specs.insert(ckey, spec.clone());
        Ok(id)
    }

    fn set_overlay_source(&self, logical_path: &str, source: String) {
        let mut inner = match lock(&self.inner) {
            Ok(g) => g,
            Err(_) => {
                log::error!("set_overlay_source: pipeline-system mutex poisoned");
                return;
            }
        };
        inner.overlays.insert(logical_path.to_string(), source);
        inner.dirty.insert(logical_path.to_string());
        log::info!("shader hot-reload: overlay source set for `{logical_path}`");
    }

    fn recompose_dirty(&self, device: &dyn GraphicsDevice) -> Result<(), RenderError> {
        let mut inner = lock(&self.inner)?;
        if inner.dirty.is_empty() {
            return Ok(());
        }
        let dirty: Vec<String> = inner.dirty.drain().collect();

        // Re-register any dirty lib modules with the composer so subsequent
        // composition picks up their new source. A lib path is one the
        // composer knows about (it has a `khora::...::` lib import path) — i.e.
        // not a pipeline entry-point name.
        for path in &dirty {
            if !inner.pipelines.contains_key(path.as_str()) {
                if let Err(e) = readd_lib_module(&mut inner, path) {
                    log::error!(
                        "shader hot-reload: failed to recompose lib `{path}`, \
                         keeping previous module: {e}"
                    );
                    // Drop the bad overlay so the next edit can recover and we
                    // don't keep re-failing on every recompose.
                    inner.overlays.remove(path);
                    continue;
                }
            }
        }

        // Compute the set of cached pipelines whose module graph includes any
        // dirty source, then rebuild each in place under the same key.
        let affected_render = affected_render_pipelines(&inner, &dirty);
        let affected_compute = affected_compute_pipelines(&inner, &dirty);

        // Drop cached module entries for every dirty name (all variants) so
        // `make_module` recomposes them on the next build.
        for path in &dirty {
            inner.modules.retain(|(name, _), _| name != path);
        }
        // A lib change forces every affected pipeline's module to recompose
        // too — drop their module-cache entries by pipeline name.
        let affected_names: HashSet<&'static str> = affected_render
            .iter()
            .map(|k| k.shader)
            .chain(affected_compute.iter().map(|k| k.shader))
            .collect();
        inner
            .modules
            .retain(|(name, _), _| !affected_names.contains(name.as_str()));

        for key in affected_render {
            rebuild_render_pipeline(&mut inner, device, &key);
        }
        for key in affected_compute {
            rebuild_compute_pipeline(&mut inner, device, &key);
        }
        Ok(())
    }
}

fn lock(m: &Mutex<Inner>) -> Result<std::sync::MutexGuard<'_, Inner>, RenderError> {
    m.lock()
        .map_err(|_| backend_err("WgpuPipelineSystem mutex poisoned"))
}

fn backend_err(msg: &str) -> RenderError {
    RenderError::ResourceError(ResourceError::BackendError(msg.to_owned()))
}

fn compose_err(path: &str, e: naga_oil::compose::ComposerError) -> RenderError {
    backend_err(&format!("naga_oil compose error in {path}: {e}"))
}

#[cfg(test)]
mod tests;
