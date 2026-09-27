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

use super::dependencies::{
    parse_imports, pipeline_depends_on, resolve_lib_source, resolve_pipeline_source,
};
use super::*;
use khora_core::renderer::api::pipeline::ShaderDefScalar;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use naga_oil::compose::{NagaModuleDescriptor, ShaderType};

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
        let _ =
            naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty())
                .unwrap_or_else(|e| panic!("emit {name}: {e}"));
    }
}

/// Composes the lit pipelines for every combination of material texture
/// variant flags, catching `#ifdef`-gated binding / import drift in the
/// material-textures lib at boot (no GPU needed). The full power set of
/// the five `HAS_*` flags exercises each gated declaration in isolation
/// and together.
#[test]
fn composes_lit_pipelines_for_texture_variants() {
    use khora_core::renderer::api::material::bindings::flag;

    let flags = [
        flag::HAS_BASE_COLOR_TEXTURE,
        flag::HAS_METALLIC_ROUGHNESS_TEXTURE,
        flag::HAS_NORMAL_MAP,
        flag::HAS_EMISSIVE_TEXTURE,
        flag::HAS_OCCLUSION_MAP,
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

/// The IBL bake shaders compose + validate (no GPU), catching WGSL errors
/// in the env-cube / irradiance bakes at boot.
#[test]
fn composes_ibl_bake_pipelines() {
    let sys = WgpuPipelineSystem::new().expect("system init");
    let mut inner = sys.inner.lock().unwrap();
    for name in [
        "khora::pipelines::ibl_sky",
        "khora::pipelines::ibl_irradiance",
        "khora::pipelines::ibl_prefilter",
        "khora::pipelines::ibl_brdf_lut",
        "khora::pipelines::skybox",
        "khora::pipelines::ibl_equirect",
    ] {
        let defs = inner.base_defs.clone();
        let source = *inner.pipelines.get(name).expect("ibl bake registered");
        let patched = inject_float_defs(source);
        let module = inner
            .composer
            .make_naga_module(NagaModuleDescriptor {
                source: &patched,
                file_path: name,
                shader_defs: defs,
                shader_type: ShaderType::Wgsl,
                ..Default::default()
            })
            .unwrap_or_else(|e| panic!("compose {name}: {e}"));
        Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(&module)
            .unwrap_or_else(|e| panic!("validate {name}: {e:?}"));
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

/// The sandbox hot-reloads every `.wgsl` under `shader/shaders/`, and the
/// watcher names a pipeline file `khora::pipelines::<stem>`. A file in
/// `pipelines/` that the table does not know is taken by `recompose_dirty`
/// for a lib module and handed to the composer. So every file in the watched
/// `pipelines/` directory must be a registered pipeline — or live elsewhere.
#[test]
fn every_watched_pipeline_file_is_a_registered_pipeline() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/graphics/shader/shaders/pipelines");
    let sys = WgpuPipelineSystem::new().expect("system init");
    let inner = sys.inner.lock().unwrap();
    let mut unregistered: Vec<String> = std::fs::read_dir(&dir)
        .expect("pipelines dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            (path.extension()? == "wgsl").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .map(|stem| format!("khora::pipelines::{stem}"))
        .filter(|name| !inner.pipelines.contains_key(name.as_str()))
        .collect();
    unregistered.sort();
    assert!(
        unregistered.is_empty(),
        "watched pipeline files the PipelineSystem does not know: {unregistered:?}"
    );
}

/// What `recompose_dirty` does, device-free, when the sandbox's watcher sees
/// an edit to `pipelines/text.wgsl`: the overlay is set under
/// `khora::pipelines::text`, which is not in the pipeline table, so it is
/// re-added to the composer as a lib module. That must not fail — an edit to
/// a file in the watched tree must not log a recompose error.
#[test]
fn hot_reload_of_text_wgsl_is_not_a_failed_lib_recompose() {
    let sys = WgpuPipelineSystem::new().expect("system init");
    let edited = format!(
        "{}\n// hot-reload overlay marker\n",
        crate::graphics::shader::TEXT_WGSL
    );
    sys.set_overlay_source("khora::pipelines::text", edited);

    let mut inner = sys.inner.lock().unwrap();
    let dirty: Vec<String> = inner.dirty.iter().cloned().collect();
    for path in &dirty {
        if !inner.pipelines.contains_key(path.as_str()) {
            readd_lib_module(&mut inner, path).unwrap_or_else(|e| {
                panic!("recompose_dirty would log `failed to recompose lib `{path}``: {e}")
            });
        }
    }
}
