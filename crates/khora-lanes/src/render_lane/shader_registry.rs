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

//! WGSL shader composition + GPU module creation, powered by
//! `naga_oil` and synced with [`khora_core::renderer::api::ShaderDefs`].
//!
//! The registry is built once at engine init: every lib module under
//! `shaders/lib/` is registered with the underlying `naga_oil::Composer`,
//! and every pipeline under `shaders/pipelines/` is pre-validated. Lane
//! code calls [`ShaderRegistry::create_module`] in its `on_initialize`
//! to materialise a `ShaderModuleId` from a pipeline name — no more
//! `include_str!` + ad-hoc concatenation.

use std::collections::HashMap;

use khora_core::renderer::api::shader_defs::ShaderDefs;
use khora_core::renderer::api::{
    core::{ShaderModuleDescriptor, ShaderModuleId, ShaderSourceData},
    util::TextureFormat,
};
use khora_core::renderer::error::RenderError;
use khora_core::renderer::GraphicsDevice;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, ImportDefinition, NagaModuleDescriptor, ShaderDefValue,
    ShaderLanguage, ShaderType,
};

use std::borrow::Cow;

// ─────────────────────────────────────────────────────────────────────
// Embedded sources
// ─────────────────────────────────────────────────────────────────────

/// Pair of (logical path, source) for a composable lib module.
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

/// Pair of (logical name, source) for a fully-composed pipeline. The
/// logical name is exposed to lane code via
/// [`ShaderRegistry::create_module`].
const PIPELINE_MODULES: &[(&str, &str)] = &[
    // Forward / lit family — uses `lib/std/*` + `lib/lighting/*` +
    // `lib/shadow/*` via `#import`.
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

    // Shadow depth-pass — uses `lib/std/{camera,model}` via `#import`.
    (
        "khora::pipelines::shadow_pass",
        include_str!("shaders/pipelines/shadow_pass.wgsl"),
    ),

    // Compute — light culling for Forward+. Self-contained.
    (
        "khora::pipelines::light_culling",
        include_str!("shaders/pipelines/light_culling.wgsl"),
    ),

    // UI / editor overlay — self-contained, validated through naga at
    // boot for consistency (no lib reuse currently).
    (
        "khora::pipelines::ui",
        include_str!("shaders/pipelines/ui.wgsl"),
    ),
    (
        "khora::pipelines::text",
        include_str!("shaders/pipelines/text.wgsl"),
    ),
    (
        "khora::pipelines::egui",
        include_str!("shaders/pipelines/egui.wgsl"),
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

// ─────────────────────────────────────────────────────────────────────
// Error type
// ─────────────────────────────────────────────────────────────────────

/// Errors produced by the shader registry.
#[derive(Debug, thiserror::Error)]
pub enum ShaderRegistryError {
    /// `naga_oil` failed to compose a module (import error, parse
    /// error, …).
    #[error("naga_oil compose error in {path}: {message}")]
    Compose {
        /// Logical path of the offending module.
        path: String,
        /// Stringified diagnostic.
        message: String,
    },
    /// `naga` failed to validate the composed module.
    #[error("naga validation error in {path}: {message}")]
    Validation {
        /// Logical path of the offending module.
        path: String,
        /// Stringified diagnostic.
        message: String,
    },
    /// The WGSL back-end failed to emit the composed module as a
    /// string.
    #[error("naga WGSL back-end error in {path}: {message}")]
    Backend {
        /// Logical path of the offending module.
        path: String,
        /// Stringified diagnostic.
        message: String,
    },
    /// A pipeline was requested but not registered.
    #[error("pipeline `{0}` not registered")]
    UnknownPipeline(String),
    /// The graphics device rejected the composed WGSL source.
    #[error(transparent)]
    Backend2(#[from] RenderError),
}

// ─────────────────────────────────────────────────────────────────────
// ShaderRegistry
// ─────────────────────────────────────────────────────────────────────

/// Shared shader compositor. Built at engine init, then queried by
/// every render lane to obtain its GPU shader module.
pub struct ShaderRegistry {
    composer: Composer,
    pipelines: HashMap<&'static str, &'static str>,
    defs: HashMap<String, ShaderDefValue>,
}

impl ShaderRegistry {
    /// Builds the registry: registers every lib module with the
    /// composer, fails fast on the first compose error. Pipelines are
    /// stored as raw sources and composed on demand.
    pub fn new() -> Result<Self, ShaderRegistryError> {
        let mut composer = Composer::default();

        // Inject `ShaderDefs` constants — every lib + pipeline sees
        // them at compose time.
        let defs: HashMap<String, ShaderDefValue> = HashMap::from([
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
            // f32 constants are emitted as their bit pattern through
            // ShaderDefValue::Int; this works because WGSL `#define`
            // substitution is textual. We pass them as the literal
            // value via custom helper below.
        ]);

        for (path, source) in LIB_MODULES {
            // Inject SHADOW_CUBE_NEAR per-module since naga_oil
            // ShaderDefValue is integer-only — we patch this constant
            // textually as a #define at the head of the source.
            let patched = inject_float_defs(source);
            composer
                .add_composable_module(ComposableModuleDescriptor {
                    source: &patched,
                    file_path: path,
                    language: ShaderLanguage::Wgsl,
                    shader_defs: defs.clone(),
                    ..Default::default()
                })
                .map_err(|e| ShaderRegistryError::Compose {
                    path: (*path).to_string(),
                    message: e.to_string(),
                })?;
        }

        let pipelines: HashMap<&'static str, &'static str> =
            PIPELINE_MODULES.iter().copied().collect();

        Ok(Self {
            composer,
            pipelines,
            defs,
        })
    }

    /// Composes the named pipeline, emits the final WGSL string, and
    /// creates a GPU shader module via `device`.
    pub fn create_module(
        &mut self,
        device: &dyn GraphicsDevice,
        pipeline_name: &str,
        label: Option<&str>,
    ) -> Result<ShaderModuleId, ShaderRegistryError> {
        let source = self
            .pipelines
            .get(pipeline_name)
            .copied()
            .ok_or_else(|| ShaderRegistryError::UnknownPipeline(pipeline_name.to_string()))?;
        let patched = inject_float_defs(source);

        // 1. Compose → naga::Module.
        let module = self
            .composer
            .make_naga_module(NagaModuleDescriptor {
                source: &patched,
                file_path: pipeline_name,
                shader_defs: self.defs.clone(),
                shader_type: ShaderType::Wgsl,
                ..Default::default()
            })
            .map_err(|e| ShaderRegistryError::Compose {
                path: pipeline_name.to_string(),
                message: e.to_string(),
            })?;

        // 2. Validate.
        let module_info = Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(&module)
            .map_err(|e| ShaderRegistryError::Validation {
                path: pipeline_name.to_string(),
                message: format!("{e:?}"),
            })?;

        // 3. Round-trip to WGSL string (so the existing
        //    `ShaderSourceData::Wgsl` pipeline stays unchanged).
        let wgsl = naga::back::wgsl::write_string(
            &module,
            &module_info,
            naga::back::wgsl::WriterFlags::empty(),
        )
        .map_err(|e| ShaderRegistryError::Backend {
            path: pipeline_name.to_string(),
            message: e.to_string(),
        })?;

        // 4. Hand off to the device.
        device
            .create_shader_module(&ShaderModuleDescriptor {
                label,
                source: ShaderSourceData::Wgsl(Cow::Owned(wgsl)),
            })
            .map_err(|e| ShaderRegistryError::Backend2(RenderError::ResourceError(e)))
    }
}

// Silence unused-import warnings for crates that pull in the registry
// transitively but don't use these directly.
#[allow(dead_code)]
const _UNUSED: Option<TextureFormat> = None;
#[allow(dead_code)]
const _UNUSED2: Option<ImportDefinition> = None;

/// `naga_oil::compose::ShaderDefValue` only carries `Int / UInt / Bool`.
/// Float constants needed in WGSL libraries (e.g. `SHADOW_CUBE_NEAR`)
/// are injected textually as `const` declarations at the head of the
/// source.
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

    /// Smoke test: every registered pipeline composes through
    /// naga_oil, passes naga validation, and round-trips back to WGSL.
    /// Catches import-graph mistakes, missing `#define_import_path`
    /// headers, and binding-layout drift at boot time — for the entire
    /// pipeline catalog, not just one.
    #[test]
    fn registry_composes_every_pipeline() {
        let mut registry = ShaderRegistry::new().expect("registry init");

        // Take ownership of the (name, source) pairs to avoid borrowing
        // `registry` while we mutate its composer inside the loop.
        let pipelines: Vec<(&'static str, &'static str)> = registry
            .pipelines
            .iter()
            .map(|(name, src)| (*name, *src))
            .collect();

        for (name, source) in pipelines {
            let patched = inject_float_defs(source);

            let module = registry
                .composer
                .make_naga_module(NagaModuleDescriptor {
                    source: &patched,
                    file_path: name,
                    shader_defs: registry.defs.clone(),
                    shader_type: ShaderType::Wgsl,
                    ..Default::default()
                })
                .unwrap_or_else(|e| panic!("compose {name}: {e}"));

            let module_info = Validator::new(ValidationFlags::all(), Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("validate {name}: {e:?}"));

            let _wgsl = naga::back::wgsl::write_string(
                &module,
                &module_info,
                naga::back::wgsl::WriterFlags::empty(),
            )
            .unwrap_or_else(|e| panic!("emit WGSL for {name}: {e}"));
        }
    }
}
