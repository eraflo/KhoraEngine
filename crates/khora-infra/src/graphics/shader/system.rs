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
use khora_core::renderer::api::core::{ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData};
use khora_core::renderer::api::pipeline::{
    ComputePipelineKey, ComputePipelineSpec, LayoutCacheKey, LayoutKey, LayoutSpec, PipelineKey,
    PipelineLayoutDescriptor, PipelineSpec, RenderPipelineDescriptor, RenderPipelineId,
    ShaderDefScalar, ShaderVariantKey,
};
use khora_core::renderer::api::shader_defs::ShaderDefs;
use khora_core::renderer::error::{RenderError, ResourceError};
use khora_core::renderer::traits::{GraphicsDevice, PipelineSystem};

use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue, ShaderLanguage,
    ShaderType,
};

/// (logical import path, source) for each composable lib module.
const LIB_MODULES: &[(&str, &str)] = &[
    (
        "khora::std::camera",
        include_str!("shaders/lib/std/camera.wgsl"),
    ),
    (
        "khora::std::model",
        include_str!("shaders/lib/std/model.wgsl"),
    ),
    (
        "khora::std::material",
        include_str!("shaders/lib/std/material.wgsl"),
    ),
    (
        "khora::std::material_textures",
        include_str!("shaders/lib/std/material_textures.wgsl"),
    ),
    (
        "khora::std::vertex",
        include_str!("shaders/lib/std/vertex.wgsl"),
    ),
    (
        "khora::lighting::structs",
        include_str!("shaders/lib/lighting/structs.wgsl"),
    ),
    (
        "khora::lighting::uniforms",
        include_str!("shaders/lib/lighting/uniforms.wgsl"),
    ),
    (
        "khora::lighting::attenuation",
        include_str!("shaders/lib/lighting/attenuation.wgsl"),
    ),
    (
        "khora::lighting::blinn_phong",
        include_str!("shaders/lib/lighting/blinn_phong.wgsl"),
    ),
    (
        "khora::shadow::bindings",
        include_str!("shaders/lib/shadow/bindings.wgsl"),
    ),
    (
        "khora::shadow::sample_2d",
        include_str!("shaders/lib/shadow/sample_2d.wgsl"),
    ),
    (
        "khora::shadow::sample_cube",
        include_str!("shaders/lib/shadow/sample_cube.wgsl"),
    ),
];

/// (logical name, source) for each composable pipeline module.
const PIPELINE_MODULES: &[(&str, &str)] = &[
    (
        "khora::pipelines::lit_forward",
        include_str!("shaders/pipelines/lit_forward.wgsl"),
    ),
    (
        "khora::pipelines::forward_plus",
        include_str!("shaders/pipelines/forward_plus.wgsl"),
    ),
    (
        "khora::pipelines::standard_pbr",
        include_str!("shaders/pipelines/standard_pbr.wgsl"),
    ),
    (
        "khora::pipelines::unlit",
        include_str!("shaders/pipelines/unlit.wgsl"),
    ),
    (
        "khora::pipelines::emissive",
        include_str!("shaders/pipelines/emissive.wgsl"),
    ),
    (
        "khora::pipelines::wireframe",
        include_str!("shaders/pipelines/wireframe.wgsl"),
    ),
    (
        "khora::pipelines::shadow_pass",
        include_str!("shaders/pipelines/shadow_pass.wgsl"),
    ),
    (
        "khora::pipelines::light_culling",
        include_str!("shaders/pipelines/light_culling.wgsl"),
    ),
    (
        "khora::pipelines::ui",
        include_str!("shaders/pipelines/ui.wgsl"),
    ),
    (
        "khora::pipelines::grid",
        include_str!("shaders/pipelines/grid.wgsl"),
    ),
    (
        "khora::pipelines::gizmo",
        include_str!("shaders/pipelines/gizmo.wgsl"),
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

/// Re-registers a dirty lib module with the composer using its overlay (or
/// embedded) source, replacing the previous registration. `naga_oil` allows
/// re-adding a composable module under the same import path.
fn readd_lib_module(inner: &mut Inner, path: &str) -> Result<(), RenderError> {
    let source = resolve_lib_source(inner, path)
        .ok_or_else(|| backend_err(&format!("lib module `{path}` has no source")))?;
    let patched = inject_float_defs(&source);
    let defs = inner.base_defs.clone();
    inner
        .composer
        .add_composable_module(ComposableModuleDescriptor {
            source: &patched,
            file_path: path,
            language: ShaderLanguage::Wgsl,
            shader_defs: defs,
            ..Default::default()
        })
        .map_err(|e| compose_err(path, e))?;
    Ok(())
}

/// The current source for a lib import path: overlay first, then the embedded
/// `include_str!` source. Returns `None` for an unknown lib path.
fn resolve_lib_source(inner: &Inner, path: &str) -> Option<String> {
    if let Some(src) = inner.overlays.get(path) {
        return Some(src.clone());
    }
    LIB_MODULES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, s)| (*s).to_string())
}

/// The current source for a pipeline module name: overlay first, then the
/// embedded source from the `pipelines` map.
fn resolve_pipeline_source(inner: &Inner, name: &str) -> Option<String> {
    if let Some(src) = inner.overlays.get(name) {
        return Some(src.clone());
    }
    inner.pipelines.get(name).map(|s| (*s).to_string())
}

/// Cached render-pipeline keys whose module graph transitively includes any of
/// the `dirty` logical names (the pipeline's own module or any imported lib).
fn affected_render_pipelines(inner: &Inner, dirty: &[String]) -> Vec<PipelineKey> {
    let dirty_set: HashSet<&str> = dirty.iter().map(String::as_str).collect();
    inner
        .pipeline_specs
        .keys()
        .filter(|key| pipeline_depends_on(inner, key.shader, &dirty_set))
        .cloned()
        .collect()
}

/// Cached compute-pipeline keys affected by any `dirty` logical name.
fn affected_compute_pipelines(inner: &Inner, dirty: &[String]) -> Vec<ComputePipelineKey> {
    let dirty_set: HashSet<&str> = dirty.iter().map(String::as_str).collect();
    inner
        .compute_pipeline_specs
        .keys()
        .filter(|key| pipeline_depends_on(inner, key.shader, &dirty_set))
        .cloned()
        .collect()
}

/// True if pipeline `name`'s module graph (its own source plus the transitive
/// closure of its `#import`ed lib modules) includes any of `dirty`.
fn pipeline_depends_on(inner: &Inner, name: &str, dirty: &HashSet<&str>) -> bool {
    if dirty.contains(name) {
        return true;
    }
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = match resolve_pipeline_source(inner, name) {
        Some(src) => parse_imports(&src),
        None => return false,
    };
    while let Some(import) = stack.pop() {
        if dirty.contains(import.as_str()) {
            return true;
        }
        if !visited.insert(import.clone()) {
            continue;
        }
        if let Some(src) = resolve_lib_source(inner, &import) {
            stack.extend(parse_imports(&src));
        }
    }
    false
}

/// Extracts the imported module paths from a WGSL source: every `#import
/// khora::a::b[::item]` yields the module path `khora::a::b` (the path minus
/// any trailing imported-item / `{...}` selector).
fn parse_imports(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("#import") else {
            continue;
        };
        let rest = rest.trim();
        // Path token ends at whitespace or the start of a `{...}` item list.
        let token: String = rest
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '{')
            .collect();
        let token = token.trim_end_matches("::");
        if token.is_empty() {
            continue;
        }
        // A `khora::std::material_textures::sample_albedo` import names a
        // function inside `khora::std::material_textures`; the module path is
        // every segment that maps to a registered lib. Map by longest known
        // lib-path prefix so item imports collapse to their module.
        out.push(longest_lib_prefix(token).unwrap_or_else(|| token.to_string()));
    }
    out
}

/// Of the registered lib import paths, the longest that is a prefix of
/// `token` (segment-wise). Falls back to `None` if none match.
fn longest_lib_prefix(token: &str) -> Option<String> {
    let mut best: Option<&str> = None;
    for (path, _) in LIB_MODULES {
        let is_prefix = token == *path || token.starts_with(&format!("{path}::"));
        if is_prefix && best.is_none_or(|b| path.len() > b.len()) {
            best = Some(path);
        }
    }
    best.map(str::to_string)
}

/// Rebuilds a single cached render pipeline in place under `key`. On a compose
/// failure (e.g. the user just typed invalid WGSL), logs the error and keeps
/// the previously-working pipeline — the cache is never poisoned.
fn rebuild_render_pipeline(inner: &mut Inner, device: &dyn GraphicsDevice, key: &PipelineKey) {
    let Some(spec) = inner.pipeline_specs.get(key).cloned() else {
        return;
    };

    let mut layout_ids: Vec<BindGroupLayoutId> = Vec::with_capacity(spec.bind_group_layouts.len());
    for ls in &spec.bind_group_layouts {
        match resolve_layout(inner, device, ls, &spec.variant) {
            Ok(id) => layout_ids.push(id),
            Err(e) => {
                log::error!(
                    "shader hot-reload: layout resolve failed for `{}`: {e}",
                    spec.label
                );
                return;
            }
        }
    }

    let pipeline_layout = match device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(Cow::Borrowed(spec.label)),
        bind_group_layouts: &layout_ids,
    }) {
        Ok(l) => l,
        Err(e) => {
            log::error!(
                "shader hot-reload: pipeline layout failed for `{}`: {e}",
                spec.label
            );
            return;
        }
    };

    let module = match make_module(inner, device, spec.shader, &spec.variant) {
        Ok(m) => m,
        Err(e) => {
            log::error!(
                "shader hot-reload: recompose failed for `{}` — keeping previous \
                 pipeline: {e}",
                spec.shader
            );
            return;
        }
    };

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
    let new_id = match device.create_render_pipeline(&desc) {
        Ok(id) => id,
        Err(e) => {
            log::error!(
                "shader hot-reload: pipeline build failed for `{}` — keeping \
                 previous pipeline: {e}",
                spec.label
            );
            return;
        }
    };

    let old_id = inner.pipeline_cache.insert(key.clone(), new_id);
    if let Some(old_id) = old_id {
        if let Err(e) = device.destroy_render_pipeline(old_id) {
            log::warn!(
                "shader hot-reload: failed to destroy old pipeline `{}`: {e}",
                spec.label
            );
        }
    }
    log::info!("shader hot-reload: rebuilt pipeline `{}`", spec.label);
}

/// Rebuilds a single cached compute pipeline in place under `key`. Same
/// keep-last-good resilience as [`rebuild_render_pipeline`].
fn rebuild_compute_pipeline(
    inner: &mut Inner,
    device: &dyn GraphicsDevice,
    key: &ComputePipelineKey,
) {
    let Some(spec) = inner.compute_pipeline_specs.get(key).cloned() else {
        return;
    };

    let mut layout_ids: Vec<BindGroupLayoutId> = Vec::with_capacity(spec.bind_group_layouts.len());
    for ls in &spec.bind_group_layouts {
        match resolve_layout(inner, device, ls, &spec.variant) {
            Ok(id) => layout_ids.push(id),
            Err(e) => {
                log::error!(
                    "shader hot-reload: layout resolve failed for `{}`: {e}",
                    spec.label
                );
                return;
            }
        }
    }

    let pipeline_layout = match device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(Cow::Borrowed(spec.label)),
        bind_group_layouts: &layout_ids,
    }) {
        Ok(l) => l,
        Err(e) => {
            log::error!(
                "shader hot-reload: pipeline layout failed for `{}`: {e}",
                spec.label
            );
            return;
        }
    };

    let module = match make_module(inner, device, spec.shader, &spec.variant) {
        Ok(m) => m,
        Err(e) => {
            log::error!(
                "shader hot-reload: recompose failed for `{}` — keeping previous \
                 compute pipeline: {e}",
                spec.shader
            );
            return;
        }
    };

    let new_id = match device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some(Cow::Borrowed(spec.label)),
        layout: Some(pipeline_layout),
        shader_module: module,
        entry_point: Cow::Borrowed(spec.entry_point),
    }) {
        Ok(id) => id,
        Err(e) => {
            log::error!(
                "shader hot-reload: compute pipeline build failed for `{}` — keeping \
                 previous: {e}",
                spec.label
            );
            return;
        }
    };

    inner.compute_pipeline_cache.insert(key.clone(), new_id);
    log::info!(
        "shader hot-reload: rebuilt compute pipeline `{}`",
        spec.label
    );
}

/// Resolves a single [`LayoutSpec`] to a cached [`BindGroupLayoutId`].
fn resolve_layout(
    inner: &mut Inner,
    device: &dyn GraphicsDevice,
    spec: &LayoutSpec,
    variant: &ShaderVariantKey,
) -> Result<BindGroupLayoutId, RenderError> {
    let cache_key = spec.cache_key(variant);
    if let Some(id) = inner.layouts.get(&cache_key) {
        return Ok(*id);
    }
    let (label, entries): (&str, Vec<_>) = match spec {
        LayoutSpec::Named(key) => (layout_label(*key), key.entries(variant)),
        LayoutSpec::Inline { label, entries } => (label, entries.to_vec()),
    };
    let id = device
        .create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &entries,
        })
        .map_err(RenderError::ResourceError)?;
    inner.layouts.insert(cache_key, id);
    Ok(id)
}

/// Composes (or reuses) the shader module for `name` under `variant`.
fn make_module(
    inner: &mut Inner,
    device: &dyn GraphicsDevice,
    name: &str,
    variant: &ShaderVariantKey,
) -> Result<ShaderModuleId, RenderError> {
    let mkey = (name.to_string(), variant.clone());
    if let Some(id) = inner.modules.get(&mkey) {
        return Ok(*id);
    }
    // Overlay (hot-reload) source wins over the embedded source.
    let source = resolve_pipeline_source(inner, name)
        .ok_or_else(|| backend_err(&format!("pipeline `{name}` not registered")))?;
    let patched = inject_float_defs(&source);

    // Global defs + per-variant overlays.
    let mut defs = inner.base_defs.clone();
    for (k, v) in variant.defs() {
        defs.insert(
            (*k).to_string(),
            match v {
                ShaderDefScalar::Bool(b) => ShaderDefValue::Bool(*b),
                ShaderDefScalar::Int(i) => ShaderDefValue::Int(*i),
                ShaderDefScalar::UInt(u) => ShaderDefValue::UInt(*u),
            },
        );
    }

    let module = inner
        .composer
        .make_naga_module(NagaModuleDescriptor {
            source: &patched,
            file_path: name,
            shader_defs: defs,
            shader_type: ShaderType::Wgsl,
            ..Default::default()
        })
        .map_err(|e| compose_err(name, e))?;

    let module_info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .map_err(|e| backend_err(&format!("naga validation error in {name}: {e:?}")))?;

    let wgsl = naga::back::wgsl::write_string(
        &module,
        &module_info,
        naga::back::wgsl::WriterFlags::empty(),
    )
    .map_err(|e| backend_err(&format!("WGSL emit error in {name}: {e}")))?;

    let id = device
        .create_shader_module(&ShaderModuleDescriptor {
            label: Some(name),
            source: ShaderSourceData::Wgsl(Cow::Owned(wgsl)),
        })
        .map_err(RenderError::ResourceError)?;
    inner.modules.insert(mkey, id);
    Ok(id)
}

fn layout_label(key: LayoutKey) -> &'static str {
    match key {
        LayoutKey::Camera => "khora_camera_layout",
        LayoutKey::Model => "khora_model_layout",
        LayoutKey::Material => "khora_material_layout",
        LayoutKey::Lighting => "khora_lighting_layout",
        LayoutKey::LightingBuffer => "khora_lighting_buffer_layout",
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

/// `naga_oil` `ShaderDefValue` is integer/bool only; float consts (e.g.
/// `SHADOW_CUBE_NEAR`) are injected textually as a `const` at the source head.
fn inject_float_defs(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 64);
    out.push_str(&format!(
        "const SHADOW_CUBE_NEAR: f32 = {};\n",
        ShaderDefs::SHADOW_CUBE_NEAR
    ));
    out.push_str(source);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: every registered pipeline composes + validates + emits WGSL
    /// at the empty variant (catches import-graph / `#define_import_path` drift
    /// at boot, no GPU needed).
    #[test]
    fn composes_every_pipeline() {
        let sys = WgpuPipelineSystem::new().expect("system init");
        let mut inner = sys.inner.lock().unwrap();
        let pipelines: Vec<(&'static str, &'static str)> =
            inner.pipelines.iter().map(|(n, s)| (*n, *s)).collect();
        let defs = inner.base_defs.clone();
        for (name, source) in pipelines {
            let patched = inject_float_defs(source);
            let module = inner
                .composer
                .make_naga_module(NagaModuleDescriptor {
                    source: &patched,
                    file_path: name,
                    shader_defs: defs.clone(),
                    shader_type: ShaderType::Wgsl,
                    ..Default::default()
                })
                .unwrap_or_else(|e| panic!("compose {name}: {e}"));
            let info = Validator::new(ValidationFlags::all(), Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("validate {name}: {e:?}"));
            let _ = naga::back::wgsl::write_string(
                &module,
                &info,
                naga::back::wgsl::WriterFlags::empty(),
            )
            .unwrap_or_else(|e| panic!("emit {name}: {e}"));
        }
    }

    /// Composes the lit pipelines for every combination of material texture
    /// variant flags, catching `#ifdef`-gated binding / import drift in the
    /// material-textures lib at boot (no GPU needed). The full power set of
    /// the four `HAS_*` flags exercises each gated declaration in isolation
    /// and together.
    #[test]
    fn composes_lit_pipelines_for_texture_variants() {
        use khora_core::renderer::api::material::bindings::flag;

        let flags = [
            flag::HAS_BASE_COLOR_TEXTURE,
            flag::HAS_METALLIC_ROUGHNESS_TEXTURE,
            flag::HAS_NORMAL_MAP,
            flag::HAS_EMISSIVE_TEXTURE,
        ];
        let lit_pipelines = [
            "khora::pipelines::lit_forward",
            "khora::pipelines::forward_plus",
            "khora::pipelines::standard_pbr",
        ];

        let sys = WgpuPipelineSystem::new().expect("system init");
        let mut inner = sys.inner.lock().unwrap();

        for mask in 0u8..(1 << flags.len()) {
            let mut variant = ShaderVariantKey::empty();
            for (bit, name) in flags.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    variant = variant.flag(name);
                }
            }
            let mut defs = inner.base_defs.clone();
            for (k, v) in variant.defs() {
                if let ShaderDefScalar::Bool(b) = v {
                    defs.insert((*k).to_string(), ShaderDefValue::Bool(*b));
                }
            }
            for name in lit_pipelines {
                let source = *inner.pipelines.get(name).expect("pipeline registered");
                let patched = inject_float_defs(source);
                let module = inner
                    .composer
                    .make_naga_module(NagaModuleDescriptor {
                        source: &patched,
                        file_path: name,
                        shader_defs: defs.clone(),
                        shader_type: ShaderType::Wgsl,
                        ..Default::default()
                    })
                    .unwrap_or_else(|e| panic!("compose {name} (mask {mask:04b}): {e}"));
                Validator::new(ValidationFlags::all(), Capabilities::all())
                    .validate(&module)
                    .unwrap_or_else(|e| panic!("validate {name} (mask {mask:04b}): {e:?}"));
            }
        }
    }

    /// `#import` lines collapse to their module path; item / `{...}` selectors
    /// and trailing `::item` are stripped to the registered lib module.
    #[test]
    fn parse_imports_collapses_to_module_paths() {
        let src = "\
#import khora::std::camera::camera
#import khora::std::vertex::{VertexInput, VertexOutput}
#import khora::std::material_textures::sample_albedo
#import khora::lighting::structs::DirectionalLight
// not an import
let x = 1;";
        let imports = parse_imports(src);
        assert!(imports.contains(&"khora::std::camera".to_string()));
        assert!(imports.contains(&"khora::std::vertex".to_string()));
        assert!(imports.contains(&"khora::std::material_textures".to_string()));
        assert!(imports.contains(&"khora::lighting::structs".to_string()));
        assert_eq!(imports.len(), 4);
    }

    /// A dirty lib that `standard_pbr` imports marks the PBR pipeline as
    /// affected; an unrelated dirty name does not.
    #[test]
    fn pipeline_dependency_tracking_is_transitive_and_precise() {
        let sys = WgpuPipelineSystem::new().expect("system init");
        let inner = sys.inner.lock().unwrap();

        let dirty_material: HashSet<&str> = ["khora::std::material_textures"].into_iter().collect();
        assert!(
            pipeline_depends_on(&inner, "khora::pipelines::standard_pbr", &dirty_material),
            "standard_pbr imports material_textures so must be affected"
        );

        // The UI pipeline does not depend on the PBR material lib.
        let dirty_pbr_lib: HashSet<&str> = ["khora::lighting::attenuation"].into_iter().collect();
        assert!(
            !pipeline_depends_on(&inner, "khora::pipelines::ui", &dirty_pbr_lib),
            "ui must not depend on a lighting lib"
        );

        // A pipeline always depends on its own module name.
        let dirty_self: HashSet<&str> = ["khora::pipelines::ui"].into_iter().collect();
        assert!(pipeline_depends_on(
            &inner,
            "khora::pipelines::ui",
            &dirty_self
        ));
    }

    /// Setting an overlay for a lib marks it dirty and re-registering it with
    /// the composer (the device-free half of `recompose_dirty`) succeeds for a
    /// trivially-different-but-valid source; a dependent pipeline still
    /// composes against the overlaid lib.
    #[test]
    fn overlay_lib_recomposes_and_dependent_pipeline_still_composes() {
        let sys = WgpuPipelineSystem::new().expect("system init");

        // A valid variant of the camera lib: original source plus a harmless
        // trailing comment, so the module is re-registered with new bytes.
        let original = LIB_MODULES
            .iter()
            .find(|(p, _)| *p == "khora::std::camera")
            .map(|(_, s)| *s)
            .expect("camera lib registered");
        let overlaid = format!("{original}\n// hot-reload overlay marker\n");

        sys.set_overlay_source("khora::std::camera", overlaid.clone());

        let mut inner = sys.inner.lock().unwrap();
        assert!(inner.dirty.contains("khora::std::camera"));
        assert_eq!(
            resolve_lib_source(&inner, "khora::std::camera").as_deref(),
            Some(overlaid.as_str())
        );

        // Re-register the overlaid lib (device-free) and confirm a dependent
        // pipeline composes against the new module.
        readd_lib_module(&mut inner, "khora::std::camera").expect("re-add overlaid lib");
        let defs = inner.base_defs.clone();
        let pbr =
            resolve_pipeline_source(&inner, "khora::pipelines::standard_pbr").expect("pbr source");
        let patched = inject_float_defs(&pbr);
        inner
            .composer
            .make_naga_module(NagaModuleDescriptor {
                source: &patched,
                file_path: "khora::pipelines::standard_pbr",
                shader_defs: defs,
                shader_type: ShaderType::Wgsl,
                ..Default::default()
            })
            .expect("standard_pbr composes against the overlaid camera lib");
    }
}
