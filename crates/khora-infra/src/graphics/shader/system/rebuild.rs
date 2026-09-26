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

//! Composing a pipeline and building it on the device.

use std::borrow::Cow;

use khora_core::renderer::api::command::{
    BindGroupLayoutDescriptor, BindGroupLayoutId, ComputePipelineDescriptor,
};
use khora_core::renderer::api::core::{ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData};
use khora_core::renderer::api::pipeline::{
    ComputePipelineKey, LayoutKey, LayoutSpec, PipelineKey, PipelineLayoutDescriptor,
    RenderPipelineDescriptor, ShaderDefScalar, ShaderVariantKey,
};
use khora_core::renderer::api::shader_defs::ShaderDefs;
use khora_core::renderer::error::RenderError;
use khora_core::renderer::traits::GraphicsDevice;

use super::dependencies::resolve_pipeline_source;
use super::{backend_err, compose_err, Inner};
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga_oil::compose::{NagaModuleDescriptor, ShaderDefValue, ShaderType};

/// Rebuilds a single cached render pipeline in place under `key`. On a compose
/// failure (e.g. the user just typed invalid WGSL), logs the error and keeps
/// the previously-working pipeline — the cache is never poisoned.
pub(super) fn rebuild_render_pipeline(
    inner: &mut Inner,
    device: &dyn GraphicsDevice,
    key: &PipelineKey,
) {
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
pub(super) fn rebuild_compute_pipeline(
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
pub(super) fn resolve_layout(
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
pub(super) fn make_module(
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

pub(super) fn layout_label(key: LayoutKey) -> &'static str {
    match key {
        LayoutKey::Camera => "khora_camera_layout",
        LayoutKey::Model => "khora_model_layout",
        LayoutKey::Material => "khora_material_layout",
        LayoutKey::Lighting => "khora_lighting_layout",
        LayoutKey::LightingBuffer => "khora_lighting_buffer_layout",
    }
}

/// `naga_oil` `ShaderDefValue` is integer/bool only; float consts (e.g.
/// `SHADOW_CUBE_NEAR`) are injected textually as a `const` at the source head.
pub(super) fn inject_float_defs(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 64);
    out.push_str(&format!(
        "const SHADOW_CUBE_NEAR: f32 = {};\n",
        ShaderDefs::SHADOW_CUBE_NEAR
    ));
    out.push_str(source);
    out
}
