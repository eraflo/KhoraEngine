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
use khora_core::renderer::api::shader::ShaderDefScalar;
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

/// Number of components a mesh vertex format feeds to the shader.
fn vertex_format_components(format: khora_core::renderer::api::pipeline::VertexFormat) -> u32 {
    use khora_core::renderer::api::pipeline::VertexFormat as F;
    match format {
        F::Float32 | F::Uint32 | F::Sint32 => 1,
        F::Float32x2 | F::Uint32x2 | F::Sint32x2 => 2,
        F::Float32x3 | F::Uint32x3 | F::Sint32x3 => 3,
        F::Float32x4 | F::Uint32x4 | F::Sint32x4 => 4,
        other => panic!("mesh vertex format {other:?} has no component count here"),
    }
}

/// Number of components of a shader input type (scalar = 1, vecN = N).
fn shader_input_components(module: &naga::Module, ty: naga::Handle<naga::Type>) -> u32 {
    match module.types[ty].inner {
        naga::TypeInner::Scalar(_) => 1,
        naga::TypeInner::Vector { size, .. } => size as u32,
        ref other => panic!("vertex input of unexpected type {other:?}"),
    }
}

/// One `@location` input of a vertex entry point, and whether the entry point
/// reads it.
struct VertexLocationInput {
    location: u32,
    components: u32,
    read: bool,
}

/// Every `@location(n)` input of the `vs_main` entry point — direct arguments
/// and members of struct arguments — with whether the function body reads it.
fn vs_main_location_inputs(module: &naga::Module) -> Vec<VertexLocationInput> {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.stage == naga::ShaderStage::Vertex && e.name == "vs_main")
        .expect("vs_main vertex entry point");
    let function = &entry.function;
    let argument_handles = |arg: u32| {
        function
            .expressions
            .iter()
            .filter(move |(_, e)| matches!(e, naga::Expression::FunctionArgument(i) if *i == arg))
            .map(|(h, _)| h)
            .collect::<Vec<_>>()
    };
    let mut inputs = Vec::new();
    for (arg_index, argument) in function.arguments.iter().enumerate() {
        let arg_index = arg_index as u32;
        match (&argument.binding, &module.types[argument.ty].inner) {
            (Some(naga::Binding::Location { location, .. }), _) => {
                inputs.push(VertexLocationInput {
                    location: *location,
                    components: shader_input_components(module, argument.ty),
                    read: !argument_handles(arg_index).is_empty(),
                })
            }
            (None, naga::TypeInner::Struct { members, .. }) => {
                let bases = argument_handles(arg_index);
                for (member_index, member) in members.iter().enumerate() {
                    if let Some(naga::Binding::Location { location, .. }) = member.binding {
                        let read = function.expressions.iter().any(|(_, e)| {
                            matches!(e, naga::Expression::AccessIndex { base, index }
                                if *index == member_index as u32 && bases.contains(base))
                        });
                        inputs.push(VertexLocationInput {
                            location,
                            components: shader_input_components(module, member.ty),
                            read,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    inputs
}

fn compose_pipeline(name: &str) -> naga::Module {
    let sys = WgpuPipelineSystem::new().expect("system init");
    let mut inner = sys.inner.lock().unwrap();
    let defs = inner.base_defs.clone();
    let source = *inner.pipelines.get(name).expect("pipeline registered");
    let patched = inject_float_defs(source);
    inner
        .composer
        .make_naga_module(NagaModuleDescriptor {
            source: &patched,
            file_path: name,
            shader_defs: defs,
            shader_type: ShaderType::Wgsl,
            ..Default::default()
        })
        .unwrap_or_else(|e| panic!("compose {name}: {e}"))
}

/// Every pipeline that draws a `Mesh` vertex buffer.
const MESH_PIPELINES: [&str; 6] = [
    "khora::pipelines::standard_pbr",
    "khora::pipelines::lit_forward",
    "khora::pipelines::forward_plus",
    "khora::pipelines::unlit",
    "khora::pipelines::wireframe",
    "khora::pipelines::shadow_pass",
];

/// A mesh pipeline's vertex shader may only ask for attributes the engine
/// mesh vertex provides, each with the width the buffer holds — otherwise the
/// pipeline either fails to build or reads garbage.
#[test]
fn mesh_pipeline_vertex_inputs_are_in_the_mesh_vertex() {
    use khora_core::renderer::api::gpu_scene::Mesh;
    for name in MESH_PIPELINES {
        let module = compose_pipeline(name);
        let inputs = vs_main_location_inputs(&module);
        assert!(!inputs.is_empty(), "{name}: vs_main has no vertex input");
        for input in inputs {
            let attribute = Mesh::VERTEX_ATTRIBUTES
                .iter()
                .find(|a| a.shader_location == input.location)
                .unwrap_or_else(|| {
                    panic!(
                        "{name}: vs_main reads @location({}) which the mesh vertex does not provide",
                        input.location
                    )
                });
            assert_eq!(
                input.components,
                vertex_format_components(attribute.format),
                "{name}: @location({}) width differs from the mesh vertex ({:?})",
                input.location,
                attribute.format
            );
        }
    }
}

/// The lit and unlit mesh pipelines take the vertex colour (location 3) and
/// use it: a mesh coloured per vertex must not render white.
#[test]
fn coloured_mesh_pipelines_read_the_vertex_colour() {
    for name in [
        "khora::pipelines::standard_pbr",
        "khora::pipelines::lit_forward",
        "khora::pipelines::forward_plus",
        "khora::pipelines::unlit",
    ] {
        let module = compose_pipeline(name);
        let inputs = vs_main_location_inputs(&module);
        let colour = inputs
            .iter()
            .find(|i| i.location == 3)
            .unwrap_or_else(|| panic!("{name}: vs_main declares no @location(3) colour input"));
        assert_eq!(colour.components, 4, "{name}: vertex colour must be a vec4");
        assert!(
            colour.read,
            "{name}: vs_main declares the vertex colour but never reads it"
        );
    }
}

// ─── KHR_materials_specular ─────────────────────────────────────────────

/// The three lit pipelines: the ones that shade with the material.
const LIT_PIPELINES: [&str; 3] = [
    "khora::pipelines::lit_forward",
    "khora::pipelines::forward_plus",
    "khora::pipelines::standard_pbr",
];

/// The group-2 binding of each material texture flag.
fn material_texture_flags() -> [(&'static str, u32); 7] {
    use khora_core::renderer::api::material::bindings::flag;
    [
        (flag::HAS_BASE_COLOR_TEXTURE, 1),
        (flag::HAS_METALLIC_ROUGHNESS_TEXTURE, 2),
        (flag::HAS_NORMAL_MAP, 3),
        (flag::HAS_EMISSIVE_TEXTURE, 4),
        (flag::HAS_OCCLUSION_MAP, 5),
        (flag::HAS_SPECULAR_MAP, 6),
        (flag::HAS_SPECULAR_COLOR_MAP, 7),
    ]
}

/// Composes and validates pipeline `name` under the material `flags`.
fn compose_with_flags(
    sys: &WgpuPipelineSystem,
    name: &str,
    flags: &[&'static str],
) -> naga::Module {
    let mut inner = sys.inner.lock().unwrap();
    let variant = flags
        .iter()
        .fold(ShaderVariantKey::empty(), |variant, flag| {
            variant.flag(flag)
        });
    let mut defs = inner.base_defs.clone();
    for (k, v) in variant.defs() {
        if let ShaderDefScalar::Bool(b) = v {
            defs.insert((*k).to_string(), ShaderDefValue::Bool(*b));
        }
    }
    let source = *inner.pipelines.get(name).expect("pipeline registered");
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
        .unwrap_or_else(|e| panic!("compose {name} ({flags:?}): {e}"));
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("validate {name} ({flags:?}): {e:?}"));
    module
}

/// The module's group-2 globals, by binding.
fn material_globals(
    module: &naga::Module,
) -> std::collections::BTreeMap<u32, naga::Handle<naga::GlobalVariable>> {
    module
        .global_variables
        .iter()
        .filter_map(|(handle, global)| match global.binding {
            Some(naga::ResourceBinding { group: 2, binding }) => Some((binding, handle)),
            _ => None,
        })
        .collect()
}

/// A function's name without naga_oil's module decoration.
fn undecorated(name: &str) -> &str {
    name.split("X_naga_oil_mod_X").next().unwrap_or(name)
}

fn function_named<'m>(module: &'m naga::Module, name: &str) -> Option<&'m naga::Function> {
    module
        .functions
        .iter()
        .map(|(_, function)| function)
        .find(|function| function.name.as_deref().map(undecorated) == Some(name))
}

fn fs_main(module: &naga::Module) -> &naga::Function {
    &module
        .entry_points
        .iter()
        .find(|e| e.stage == naga::ShaderStage::Fragment && e.name == "fs_main")
        .expect("fs_main fragment entry point")
        .function
}

/// Whether `function`, or a function it calls for a value, has an expression
/// matching `found`.
fn reaches(
    module: &naga::Module,
    function: &naga::Function,
    found: &dyn Fn(&naga::Function, &naga::Expression) -> bool,
    seen: &mut HashSet<naga::Handle<naga::Function>>,
) -> bool {
    function.expressions.iter().any(|(_, expression)| {
        found(function, expression)
            || match expression {
                naga::Expression::CallResult(callee) => {
                    seen.insert(*callee) && reaches(module, &module.functions[*callee], found, seen)
                }
                _ => false,
            }
    })
}

/// Whether `fs_main` reads `global`, directly or through the functions it calls.
fn fragment_reads_global(
    module: &naga::Module,
    global: naga::Handle<naga::GlobalVariable>,
) -> bool {
    let found = |_: &naga::Function, e: &naga::Expression| matches!(e, naga::Expression::GlobalVariable(g) if *g == global);
    reaches(module, fs_main(module), &found, &mut HashSet::new())
}

/// Whether `expression` of `function` is `global` (or a load of it).
fn is_global(
    function: &naga::Function,
    expression: naga::Handle<naga::Expression>,
    global: naga::Handle<naga::GlobalVariable>,
) -> bool {
    match function.expressions[expression] {
        naga::Expression::GlobalVariable(g) => g == global,
        naga::Expression::Load { pointer } => is_global(function, pointer, global),
        _ => false,
    }
}

/// Whether `fs_main` reads member `member` of the struct `global`, directly or
/// through the functions it calls.
fn fragment_reads_member(
    module: &naga::Module,
    global: naga::Handle<naga::GlobalVariable>,
    member: u32,
) -> bool {
    let found = |function: &naga::Function, e: &naga::Expression| {
        matches!(e, naga::Expression::AccessIndex { base, index }
            if *index == member && is_global(function, *base, global))
    };
    reaches(module, fs_main(module), &found, &mut HashSet::new())
}

/// Each specular map is declared, at its binding, exactly when its flag is
/// set, and the fragment samples it then; the sampler sits at 8 in every
/// variant. Each new flag alone, every flag together, and none.
#[test]
fn lit_pipelines_bind_and_sample_each_specular_map_only_under_its_flag() {
    use khora_core::renderer::api::material::bindings::flag;

    let every_flag: Vec<&'static str> = material_texture_flags()
        .iter()
        .map(|(flag, _)| *flag)
        .collect();
    let variants: [Vec<&'static str>; 4] = [
        vec![],
        vec![flag::HAS_SPECULAR_MAP],
        vec![flag::HAS_SPECULAR_COLOR_MAP],
        every_flag,
    ];
    let sys = WgpuPipelineSystem::new().expect("system init");
    for name in LIT_PIPELINES {
        for flags in &variants {
            let module = compose_with_flags(&sys, name, flags);
            let globals = material_globals(&module);
            let what = format!(
                "{name} {flags:?}: group 2 = {:?}",
                globals.keys().collect::<Vec<_>>()
            );

            // naga_oil drops the globals a variant never reads, so the sampler
            // is only in a module that samples some map; wherever it is, it is
            // at binding 8 and nowhere else.
            for (binding, global) in &globals {
                let is_sampler = matches!(
                    module.types[module.global_variables[*global].ty].inner,
                    naga::TypeInner::Sampler { .. }
                );
                assert_eq!(
                    is_sampler,
                    *binding == 8,
                    "{what}: the sampler belongs at binding 8, found binding {binding}"
                );
            }
            if !flags.is_empty() {
                assert!(
                    globals.contains_key(&8),
                    "{what}: a variant sampling a map has no sampler at binding 8"
                );
            }

            for (flag, binding) in material_texture_flags() {
                let set = flags.contains(&flag);
                match globals.get(&binding) {
                    Some(texture) if set => {
                        assert!(
                            matches!(
                                module.types[module.global_variables[*texture].ty].inner,
                                naga::TypeInner::Image { .. }
                            ),
                            "{what}: binding {binding} ({flag}) is not a texture"
                        );
                        assert!(
                            fragment_reads_global(&module, *texture),
                            "{what}: fs_main never samples {flag} at binding {binding}"
                        );
                    }
                    None if set => panic!("{what}: {flag} set but nothing at binding {binding}"),
                    Some(_) => panic!("{what}: binding {binding} declared without {flag}"),
                    None => {}
                }
            }

            let sampled_textures = module
                .global_variables
                .iter()
                .filter(|(_, g)| matches!(module.types[g.ty].inner, naga::TypeInner::Image { .. }))
                .count();
            assert!(
                sampled_textures <= 16,
                "{what}: {sampled_textures} sampled textures, over the 16 per stage"
            );
        }
    }
}

/// The WGSL `MaterialUniforms` is the Rust one byte for byte: 80 bytes, the
/// specular vec4 at 64, after `pbr_factors`.
#[test]
fn the_wgsl_material_uniforms_mirror_the_rust_layout() {
    use khora_core::renderer::api::material::MaterialUniforms;

    assert_eq!(std::mem::size_of::<MaterialUniforms>(), 80);
    assert_eq!(std::mem::offset_of!(MaterialUniforms, pbr_factors), 48);
    assert_eq!(std::mem::offset_of!(MaterialUniforms, specular), 64);

    let sys = WgpuPipelineSystem::new().expect("system init");
    for name in LIT_PIPELINES {
        let module = compose_with_flags(&sys, name, &[]);
        let uniform = *material_globals(&module)
            .get(&0)
            .unwrap_or_else(|| panic!("{name}: no material uniform at binding 0"));
        let naga::TypeInner::Struct { ref members, span } =
            module.types[module.global_variables[uniform].ty].inner
        else {
            panic!("{name}: the material uniform is not a struct");
        };
        assert_eq!(
            span as usize,
            std::mem::size_of::<MaterialUniforms>(),
            "{name}: WGSL MaterialUniforms size"
        );
        let member = |field: &str| {
            members
                .iter()
                .find(|m| m.name.as_deref() == Some(field))
                .unwrap_or_else(|| panic!("{name}: MaterialUniforms has no `{field}`"))
        };
        assert_eq!(
            member("pbr_factors").offset as usize,
            std::mem::offset_of!(MaterialUniforms, pbr_factors),
            "{name}: pbr_factors offset"
        );
        let specular = member("specular");
        assert_eq!(
            specular.offset as usize,
            std::mem::offset_of!(MaterialUniforms, specular),
            "{name}: specular offset"
        );
        assert!(
            matches!(
                module.types[specular.ty].inner,
                naga::TypeInner::Vector {
                    size: naga::VectorSize::Quad,
                    scalar: naga::Scalar::F32
                }
            ),
            "{name}: specular is a vec4<f32>"
        );
    }
}

/// Every lit fragment shades with the material's specular colour and
/// strength, with or without specular maps.
#[test]
fn lit_fragments_read_the_material_specular_factors() {
    let sys = WgpuPipelineSystem::new().expect("system init");
    for name in LIT_PIPELINES {
        let module = compose_with_flags(&sys, name, &[]);
        let uniform = *material_globals(&module)
            .get(&0)
            .unwrap_or_else(|| panic!("{name}: no material uniform at binding 0"));
        let naga::TypeInner::Struct { ref members, .. } =
            module.types[module.global_variables[uniform].ty].inner
        else {
            panic!("{name}: the material uniform is not a struct");
        };
        let specular = members
            .iter()
            .position(|m| m.name.as_deref() == Some("specular"))
            .unwrap_or_else(|| panic!("{name}: MaterialUniforms has no `specular`"));
        assert!(
            fragment_reads_member(&module, uniform, specular as u32),
            "{name}: fs_main never reads material.specular"
        );
    }
}

/// The shared Fresnel term takes the F90 the specular strength sets:
/// `fresnel_schlick(cos, f0, f90)`.
#[test]
fn fresnel_schlick_takes_an_f90() {
    let sys = WgpuPipelineSystem::new().expect("system init");
    for name in LIT_PIPELINES {
        let module = compose_with_flags(&sys, name, &[]);
        let fresnel = function_named(&module, "fresnel_schlick")
            .unwrap_or_else(|| panic!("{name}: no fresnel_schlick in the composed module"));
        assert_eq!(
            fresnel.arguments.len(),
            3,
            "{name}: fresnel_schlick(cos_theta, f0, f90)"
        );
    }
}

/// `sample_specular` reads the strength map as a scalar,
/// `sample_specular_color` the colour map as rgb.
#[test]
fn material_textures_sample_specular_as_a_scalar_and_specular_colour_as_rgb() {
    use khora_core::renderer::api::material::bindings::flag;

    let sys = WgpuPipelineSystem::new().expect("system init");
    for name in LIT_PIPELINES {
        let module = compose_with_flags(
            &sys,
            name,
            &[flag::HAS_SPECULAR_MAP, flag::HAS_SPECULAR_COLOR_MAP],
        );
        let result = |function: &str| {
            let found = function_named(&module, function)
                .unwrap_or_else(|| panic!("{name}: no {function} in the composed module"));
            let result = found
                .result
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: {function} returns nothing"));
            module.types[result.ty].inner.clone()
        };
        assert!(
            matches!(
                result("sample_specular"),
                naga::TypeInner::Scalar(naga::Scalar::F32)
            ),
            "{name}: sample_specular returns an f32"
        );
        assert!(
            matches!(
                result("sample_specular_color"),
                naga::TypeInner::Vector {
                    size: naga::VectorSize::Tri,
                    scalar: naga::Scalar::F32
                }
            ),
            "{name}: sample_specular_color returns a vec3<f32>"
        );
    }
}

/// Each call's result expression, with the arguments it was called with.
type Calls =
    std::collections::HashMap<naga::Handle<naga::Expression>, Vec<naga::Handle<naga::Expression>>>;

/// The `Call` statements of `block`, nested blocks included: each call's
/// result expression with the arguments it was computed from.
fn call_arguments(block: &naga::Block, calls: &mut Calls) {
    for statement in block.iter() {
        match statement {
            naga::Statement::Call {
                arguments,
                result: Some(result),
                ..
            } => {
                calls.insert(*result, arguments.clone());
            }
            naga::Statement::Block(inner) => call_arguments(inner, calls),
            naga::Statement::If { accept, reject, .. } => {
                call_arguments(accept, calls);
                call_arguments(reject, calls);
            }
            naga::Statement::Loop {
                body, continuing, ..
            } => {
                call_arguments(body, calls);
                call_arguments(continuing, calls);
            }
            _ => {}
        }
    }
}

/// The expressions `expression` is computed from, a call's result included
/// (through the arguments it was called with).
fn operands(
    function: &naga::Function,
    calls: &Calls,
    expression: naga::Handle<naga::Expression>,
) -> Vec<naga::Handle<naga::Expression>> {
    use naga::Expression as E;
    match &function.expressions[expression] {
        E::Binary { left, right, .. } => vec![*left, *right],
        E::Unary { expr, .. } | E::Splat { value: expr, .. } | E::Load { pointer: expr } => {
            vec![*expr]
        }
        E::AccessIndex { base, .. } => vec![*base],
        E::Access { base, index } => vec![*base, *index],
        E::Swizzle { vector, .. } => vec![*vector],
        E::Compose { components, .. } => components.clone(),
        E::Math {
            arg,
            arg1,
            arg2,
            arg3,
            ..
        } => [Some(*arg), *arg1, *arg2, *arg3]
            .into_iter()
            .flatten()
            .collect(),
        E::Select {
            condition,
            accept,
            reject,
        } => vec![*condition, *accept, *reject],
        E::As { expr, .. } => vec![*expr],
        E::CallResult(_) => calls.get(&expression).cloned().unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Whether `expression` is computed, at any depth, from `found`.
fn computed_from(
    function: &naga::Function,
    calls: &Calls,
    expression: naga::Handle<naga::Expression>,
    found: &dyn Fn(naga::Handle<naga::Expression>) -> bool,
) -> bool {
    found(expression)
        || operands(function, calls, expression)
            .into_iter()
            .any(|operand| computed_from(function, calls, operand, found))
}

/// Whether `expression` reaches `target` along some path that is never
/// multiplied by a value computed from `weight`.
fn reaches_unweighted(
    function: &naga::Function,
    calls: &Calls,
    expression: naga::Handle<naga::Expression>,
    target: &dyn Fn(naga::Handle<naga::Expression>) -> bool,
    weight: &dyn Fn(naga::Handle<naga::Expression>) -> bool,
) -> bool {
    if target(expression) {
        return true;
    }
    // A product weights the paths through one factor by the other factor.
    if let naga::Expression::Binary {
        op: naga::BinaryOperator::Multiply,
        left,
        right,
    } = function.expressions[expression]
    {
        let through = |factor, other| {
            !computed_from(function, calls, other, weight)
                && reaches_unweighted(function, calls, factor, target, weight)
        };
        return through(left, right) || through(right, left);
    }
    operands(function, calls, expression)
        .into_iter()
        .any(|operand| reaches_unweighted(function, calls, operand, target, weight))
}

/// A specular strength of 0 removes a non-metal's reflection, the
/// environment's included: in the split-sum `f0·A + f90·B`, the LUT's bias
/// `B` (`brdf.y`) is scaled by the grazing reflectance as the scale is, so no
/// path carries `brdf.y` to the ambient result without a factor computed
/// from the dielectric reflectance. At the default strength that factor is
/// exactly 1, so today's image is unchanged.
#[test]
fn the_ibl_split_sum_bias_is_scaled_by_the_specular_strength() {
    let sys = WgpuPipelineSystem::new().expect("system init");
    for name in LIT_PIPELINES {
        let module = compose_with_flags(&sys, name, &[]);
        let ibl = function_named(&module, "ibl_ambient")
            .unwrap_or_else(|| panic!("{name}: no ibl_ambient in the composed module"));
        let argument = |wanted: &str| {
            ibl.arguments
                .iter()
                .position(|a| a.name.as_deref() == Some(wanted))
                .unwrap_or_else(|| panic!("{name}: ibl_ambient has no `{wanted}` argument"))
                as u32
        };
        let (brdf, dielectric) = (argument("brdf"), argument("dielectric"));
        let mut calls = std::collections::HashMap::new();
        call_arguments(&ibl.body, &mut calls);

        let is_bias = |e: naga::Handle<naga::Expression>| {
            matches!(ibl.expressions[e], naga::Expression::AccessIndex { base, index: 1 }
                if matches!(ibl.expressions[base], naga::Expression::FunctionArgument(i) if i == brdf))
        };
        let is_dielectric = |e: naga::Handle<naga::Expression>| matches!(ibl.expressions[e], naga::Expression::FunctionArgument(i) if i == dielectric);
        let result = ibl
            .body
            .iter()
            .find_map(|statement| match statement {
                naga::Statement::Return { value: Some(value) } => Some(*value),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name}: ibl_ambient returns nothing"));

        assert!(
            computed_from(ibl, &calls, result, &is_bias),
            "{name}: ibl_ambient does not read the LUT bias brdf.y at all"
        );
        assert!(
            !reaches_unweighted(ibl, &calls, result, &is_bias, &is_dielectric),
            "{name}: the LUT bias brdf.y reaches the ambient unscaled by the specular \
             strength — a dielectric of specular 0 still reflects prefiltered * brdf.y"
        );
    }
}

// ─── The UI quad's corners, evaluated on the composed vertex shader ─────────
//
// The UI lane draws each quad as a four-vertex triangle strip, so vertex `i`
// of an instance must land on corner (0,0), (1,0), (0,1), (1,1) in that order.
// Rather than pin the WGSL text (which any equivalent rewrite would break), the
// composed `vs_main` is run on the CPU by the small evaluator below, for a
// mock instance at the origin of size 1 whose UVs span [0, 1] and an identity
// projection: the vertex's UV and its clip-space position are then the corner
// itself.

/// A value of the vertex-shader evaluator.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Bool(bool),
    U32(u32),
    I32(i32),
    F32(f32),
    /// A vector, a matrix (its columns), a struct or a fixed array.
    Composite(Vec<Val>),
    /// A runtime-sized array: every element is the same.
    Repeat(Box<Val>),
    /// A pointer: a root variable and the indices walked from it.
    Ptr(Root, Vec<usize>),
    /// Anything the evaluator does not model (texture samples, …).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Root {
    Local(naga::Handle<naga::LocalVariable>),
    Global(naga::Handle<naga::GlobalVariable>),
}

/// How a block ended.
enum Flow {
    Next,
    Break,
    Return(Val),
}

/// Runs one entry point of a composed module on the CPU, straight-line code
/// and branches only.
struct VertexEval<'m> {
    module: &'m naga::Module,
    function: &'m naga::Function,
    args: Vec<Val>,
    locals: std::collections::HashMap<naga::Handle<naga::LocalVariable>, Val>,
    globals: std::collections::HashMap<naga::Handle<naga::GlobalVariable>, Val>,
    cache: std::collections::HashMap<naga::Handle<naga::Expression>, Val>,
}

fn scalar_zero(kind: naga::ScalarKind) -> Val {
    match kind {
        naga::ScalarKind::Float | naga::ScalarKind::AbstractFloat => Val::F32(0.0),
        naga::ScalarKind::Uint => Val::U32(0),
        naga::ScalarKind::Sint | naga::ScalarKind::AbstractInt => Val::I32(0),
        naga::ScalarKind::Bool => Val::Bool(false),
    }
}

/// A value of type `ty`: zero everywhere when `mock` is false; otherwise the
/// mock inputs — matrices are the identity, and a vector named `size` or
/// `uv_max` is all ones.
fn value_of(
    module: &naga::Module,
    ty: naga::Handle<naga::Type>,
    name: Option<&str>,
    mock: bool,
) -> Val {
    match &module.types[ty].inner {
        naga::TypeInner::Scalar(scalar) => scalar_zero(scalar.kind),
        naga::TypeInner::Vector { size, scalar } => {
            let ones = mock && matches!(name, Some("size" | "uv_max"));
            let component = match (ones, scalar_zero(scalar.kind)) {
                (true, Val::F32(_)) => Val::F32(1.0),
                (_, zero) => zero,
            };
            Val::Composite(vec![component; *size as usize])
        }
        naga::TypeInner::Matrix { columns, rows, .. } => Val::Composite(
            (0..*columns as usize)
                .map(|c| {
                    Val::Composite(
                        (0..*rows as usize)
                            .map(|r| Val::F32(if mock && r == c { 1.0 } else { 0.0 }))
                            .collect(),
                    )
                })
                .collect(),
        ),
        naga::TypeInner::Struct { members, .. } => Val::Composite(
            members
                .iter()
                .map(|m| value_of(module, m.ty, m.name.as_deref(), mock))
                .collect(),
        ),
        naga::TypeInner::Array { base, size, .. } => match size {
            naga::ArraySize::Constant(n) => {
                Val::Composite(vec![value_of(module, *base, None, mock); n.get() as usize])
            }
            _ => Val::Repeat(Box::new(value_of(module, *base, None, mock))),
        },
        _ => Val::Unknown,
    }
}

fn index_into(value: Val, index: usize) -> Val {
    match value {
        Val::Ptr(root, mut path) => {
            path.push(index);
            Val::Ptr(root, path)
        }
        Val::Composite(items) => items.get(index).cloned().unwrap_or(Val::Unknown),
        Val::Repeat(item) => *item,
        _ => Val::Unknown,
    }
}

fn as_index(value: &Val) -> Option<usize> {
    match value {
        Val::U32(i) => Some(*i as usize),
        Val::I32(i) if *i >= 0 => Some(*i as usize),
        _ => None,
    }
}

fn scalar_binary(op: naga::BinaryOperator, left: &Val, right: &Val) -> Val {
    use naga::BinaryOperator as B;
    match (left, right) {
        (Val::F32(a), Val::F32(b)) => match op {
            B::Add => Val::F32(a + b),
            B::Subtract => Val::F32(a - b),
            B::Multiply => Val::F32(a * b),
            B::Divide => Val::F32(a / b),
            B::Equal => Val::Bool(a == b),
            B::NotEqual => Val::Bool(a != b),
            B::Less => Val::Bool(a < b),
            B::LessEqual => Val::Bool(a <= b),
            B::Greater => Val::Bool(a > b),
            B::GreaterEqual => Val::Bool(a >= b),
            _ => Val::Unknown,
        },
        (Val::U32(a), Val::U32(b)) => match op {
            B::Add => Val::U32(a.wrapping_add(*b)),
            B::Subtract => Val::U32(a.wrapping_sub(*b)),
            B::Multiply => Val::U32(a.wrapping_mul(*b)),
            B::Divide if *b != 0 => Val::U32(a / b),
            B::Modulo if *b != 0 => Val::U32(a % b),
            B::Equal => Val::Bool(a == b),
            B::NotEqual => Val::Bool(a != b),
            B::Less => Val::Bool(a < b),
            B::LessEqual => Val::Bool(a <= b),
            B::Greater => Val::Bool(a > b),
            B::GreaterEqual => Val::Bool(a >= b),
            B::And => Val::U32(a & b),
            B::InclusiveOr => Val::U32(a | b),
            B::ExclusiveOr => Val::U32(a ^ b),
            B::ShiftLeft => Val::U32(a.wrapping_shl(*b)),
            B::ShiftRight => Val::U32(a.wrapping_shr(*b)),
            _ => Val::Unknown,
        },
        (Val::I32(a), Val::I32(b)) => match op {
            B::Add => Val::I32(a.wrapping_add(*b)),
            B::Subtract => Val::I32(a.wrapping_sub(*b)),
            B::Multiply => Val::I32(a.wrapping_mul(*b)),
            B::Divide if *b != 0 => Val::I32(a / b),
            B::Modulo if *b != 0 => Val::I32(a % b),
            B::Equal => Val::Bool(a == b),
            B::NotEqual => Val::Bool(a != b),
            B::Less => Val::Bool(a < b),
            B::LessEqual => Val::Bool(a <= b),
            B::Greater => Val::Bool(a > b),
            B::GreaterEqual => Val::Bool(a >= b),
            B::And => Val::I32(a & b),
            B::InclusiveOr => Val::I32(a | b),
            B::ExclusiveOr => Val::I32(a ^ b),
            _ => Val::Unknown,
        },
        (Val::I32(a), Val::U32(b)) => match op {
            B::ShiftLeft => Val::I32(a.wrapping_shl(*b)),
            B::ShiftRight => Val::I32(a.wrapping_shr(*b)),
            _ => Val::Unknown,
        },
        (Val::Bool(a), Val::Bool(b)) => match op {
            B::LogicalAnd | B::And => Val::Bool(*a && *b),
            B::LogicalOr | B::InclusiveOr => Val::Bool(*a || *b),
            B::Equal => Val::Bool(a == b),
            B::NotEqual => Val::Bool(a != b),
            _ => Val::Unknown,
        },
        _ => Val::Unknown,
    }
}

fn is_matrix(value: &Val) -> bool {
    matches!(value, Val::Composite(columns) if matches!(columns.first(), Some(Val::Composite(_))))
}

fn binary(op: naga::BinaryOperator, left: Val, right: Val) -> Val {
    match (left, right) {
        (Val::Unknown, _) | (_, Val::Unknown) => Val::Unknown,
        // matrix × vector: the sum of the columns weighted by the vector.
        (matrix, Val::Composite(vector))
            if op == naga::BinaryOperator::Multiply
                && is_matrix(&matrix)
                && !vector.iter().any(|v| matches!(v, Val::Composite(_))) =>
        {
            let Val::Composite(columns) = matrix else {
                return Val::Unknown;
            };
            let mut sum: Option<Val> = None;
            for (column, weight) in columns.into_iter().zip(vector) {
                let term = binary(naga::BinaryOperator::Multiply, column, weight);
                sum = Some(match sum {
                    None => term,
                    Some(acc) => binary(naga::BinaryOperator::Add, acc, term),
                });
            }
            sum.unwrap_or(Val::Unknown)
        }
        (Val::Composite(a), Val::Composite(b)) if a.len() == b.len() => Val::Composite(
            a.into_iter()
                .zip(b)
                .map(|(x, y)| binary(op, x, y))
                .collect(),
        ),
        (Val::Composite(a), scalar) => Val::Composite(
            a.into_iter()
                .map(|x| binary(op, x, scalar.clone()))
                .collect(),
        ),
        (scalar, Val::Composite(b)) => Val::Composite(
            b.into_iter()
                .map(|y| binary(op, scalar.clone(), y))
                .collect(),
        ),
        (a, b) => scalar_binary(op, &a, &b),
    }
}

fn cast(value: Val, kind: naga::ScalarKind, convert: bool) -> Val {
    use naga::ScalarKind as K;
    match value {
        Val::Composite(items) => {
            Val::Composite(items.into_iter().map(|v| cast(v, kind, convert)).collect())
        }
        Val::F32(f) => match (kind, convert) {
            (K::Float, _) => Val::F32(f),
            (K::Uint, true) => Val::U32(f as u32),
            (K::Uint, false) => Val::U32(f.to_bits()),
            (K::Sint, true) => Val::I32(f as i32),
            (K::Sint, false) => Val::I32(f.to_bits() as i32),
            (K::Bool, _) => Val::Bool(f != 0.0),
            _ => Val::Unknown,
        },
        Val::U32(u) => match (kind, convert) {
            (K::Float, true) => Val::F32(u as f32),
            (K::Float, false) => Val::F32(f32::from_bits(u)),
            (K::Uint, _) => Val::U32(u),
            (K::Sint, _) => Val::I32(u as i32),
            (K::Bool, _) => Val::Bool(u != 0),
            _ => Val::Unknown,
        },
        Val::I32(i) => match (kind, convert) {
            (K::Float, true) => Val::F32(i as f32),
            (K::Float, false) => Val::F32(f32::from_bits(i as u32)),
            (K::Uint, _) => Val::U32(i as u32),
            (K::Sint, _) => Val::I32(i),
            (K::Bool, _) => Val::Bool(i != 0),
            _ => Val::Unknown,
        },
        Val::Bool(b) => match kind {
            K::Float => Val::F32(if b { 1.0 } else { 0.0 }),
            K::Uint => Val::U32(u32::from(b)),
            K::Sint => Val::I32(i32::from(b)),
            K::Bool => Val::Bool(b),
            _ => Val::Unknown,
        },
        _ => Val::Unknown,
    }
}

/// `f` applied component by component over up to three operands, broadcasting
/// a scalar against a vector.
fn componentwise(args: &[Val], f: &dyn Fn(&[f32]) -> f32) -> Val {
    let width = args.iter().find_map(|a| match a {
        Val::Composite(items) => Some(items.len()),
        _ => None,
    });
    let component = |arg: &Val, i: usize| -> Option<f32> {
        match arg {
            Val::F32(f) => Some(*f),
            Val::Composite(items) => match items.get(i) {
                Some(Val::F32(f)) => Some(*f),
                _ => None,
            },
            _ => None,
        }
    };
    let at = |i: usize| -> Option<Val> {
        let operands: Option<Vec<f32>> = args.iter().map(|a| component(a, i)).collect();
        operands.map(|o| Val::F32(f(&o)))
    };
    match width {
        None => at(0).unwrap_or(Val::Unknown),
        Some(n) => (0..n)
            .map(at)
            .collect::<Option<Vec<Val>>>()
            .map(Val::Composite)
            .unwrap_or(Val::Unknown),
    }
}

impl<'m> VertexEval<'m> {
    fn new(module: &'m naga::Module, entry: &'m naga::EntryPoint, vertex_index: u32) -> Self {
        let function = &entry.function;
        let builtin = |binding: &Option<naga::Binding>| match binding {
            Some(naga::Binding::BuiltIn(naga::BuiltIn::VertexIndex)) => {
                Some(Val::U32(vertex_index))
            }
            Some(naga::Binding::BuiltIn(naga::BuiltIn::InstanceIndex)) => Some(Val::U32(0)),
            _ => None,
        };
        let args = function
            .arguments
            .iter()
            .map(|arg| match builtin(&arg.binding) {
                Some(v) => v,
                None => match &module.types[arg.ty].inner {
                    naga::TypeInner::Struct { members, .. } => Val::Composite(
                        members
                            .iter()
                            .map(|m| {
                                builtin(&m.binding)
                                    .unwrap_or_else(|| value_of(module, m.ty, None, false))
                            })
                            .collect(),
                    ),
                    _ => value_of(module, arg.ty, None, false),
                },
            })
            .collect();
        let globals = module
            .global_variables
            .iter()
            .map(|(h, g)| (h, value_of(module, g.ty, g.name.as_deref(), true)))
            .collect();
        let mut eval = Self {
            module,
            function,
            args,
            locals: std::collections::HashMap::new(),
            globals,
            cache: std::collections::HashMap::new(),
        };
        for (h, local) in function.local_variables.iter() {
            let value = match local.init {
                Some(init) => eval.eval(init),
                None => value_of(module, local.ty, None, false),
            };
            eval.locals.insert(h, value);
        }
        eval
    }

    fn read(&self, root: Root, path: &[usize]) -> Val {
        let mut value = match root {
            Root::Local(h) => self.locals.get(&h).cloned().unwrap_or(Val::Unknown),
            Root::Global(h) => self.globals.get(&h).cloned().unwrap_or(Val::Unknown),
        };
        for &i in path {
            value = index_into(value, i);
        }
        value
    }

    fn write(&mut self, pointer: Val, value: Val) {
        let Val::Ptr(Root::Local(h), path) = pointer else {
            return;
        };
        let Some(mut slot) = self.locals.get_mut(&h) else {
            return;
        };
        for &i in &path {
            slot = match slot {
                Val::Composite(items) => match items.get_mut(i) {
                    Some(item) => item,
                    None => return,
                },
                _ => return,
            };
        }
        *slot = value;
    }

    fn eval(&mut self, h: naga::Handle<naga::Expression>) -> Val {
        if let Some(v) = self.cache.get(&h) {
            return v.clone();
        }
        let value = match &self.function.expressions[h] {
            naga::Expression::Literal(literal) => match *literal {
                naga::Literal::F32(f) => Val::F32(f),
                naga::Literal::F64(f) => Val::F32(f as f32),
                naga::Literal::U32(u) => Val::U32(u),
                naga::Literal::I32(i) => Val::I32(i),
                naga::Literal::Bool(b) => Val::Bool(b),
                naga::Literal::AbstractInt(i) => Val::I32(i as i32),
                naga::Literal::AbstractFloat(f) => Val::F32(f as f32),
                _ => Val::Unknown,
            },
            naga::Expression::ZeroValue(ty) => value_of(self.module, *ty, None, false),
            naga::Expression::Compose { ty, components } => {
                let values: Vec<Val> = components
                    .clone()
                    .into_iter()
                    .map(|c| self.eval(c))
                    .collect();
                match &self.module.types[*ty].inner {
                    // `vec4(v.xy, 0.0, 1.0)`: vector components flatten.
                    naga::TypeInner::Vector { .. } => Val::Composite(
                        values
                            .into_iter()
                            .flat_map(|v| match v {
                                Val::Composite(items) => items,
                                scalar => vec![scalar],
                            })
                            .collect(),
                    ),
                    _ => Val::Composite(values),
                }
            }
            naga::Expression::Access { base, index } => {
                let (base, index) = (*base, *index);
                let base = self.eval(base);
                match as_index(&self.eval(index)) {
                    Some(i) => index_into(base, i),
                    None => Val::Unknown,
                }
            }
            naga::Expression::AccessIndex { base, index } => {
                let index = *index as usize;
                let base = self.eval(*base);
                index_into(base, index)
            }
            naga::Expression::Splat { size, value } => {
                let size = *size as usize;
                Val::Composite(vec![self.eval(*value); size])
            }
            naga::Expression::Swizzle {
                size,
                vector,
                pattern,
            } => {
                let (size, pattern) = (*size as usize, *pattern);
                match self.eval(*vector) {
                    Val::Composite(items) => Val::Composite(
                        pattern[..size]
                            .iter()
                            .map(|c| items.get(*c as usize).cloned().unwrap_or(Val::Unknown))
                            .collect(),
                    ),
                    _ => Val::Unknown,
                }
            }
            naga::Expression::FunctionArgument(i) => {
                self.args.get(*i as usize).cloned().unwrap_or(Val::Unknown)
            }
            naga::Expression::GlobalVariable(g) => Val::Ptr(Root::Global(*g), Vec::new()),
            naga::Expression::LocalVariable(l) => Val::Ptr(Root::Local(*l), Vec::new()),
            naga::Expression::Load { pointer } => match self.eval(*pointer) {
                Val::Ptr(root, path) => self.read(root, &path),
                _ => Val::Unknown,
            },
            naga::Expression::Unary { op, expr } => {
                let op = *op;
                match (op, self.eval(*expr)) {
                    (naga::UnaryOperator::Negate, Val::F32(f)) => Val::F32(-f),
                    (naga::UnaryOperator::Negate, Val::I32(i)) => Val::I32(-i),
                    (naga::UnaryOperator::LogicalNot, Val::Bool(b)) => Val::Bool(!b),
                    (naga::UnaryOperator::BitwiseNot, Val::U32(u)) => Val::U32(!u),
                    (naga::UnaryOperator::BitwiseNot, Val::I32(i)) => Val::I32(!i),
                    _ => Val::Unknown,
                }
            }
            naga::Expression::Binary { op, left, right } => {
                let (op, left, right) = (*op, *left, *right);
                let left = self.eval(left);
                let right = self.eval(right);
                binary(op, left, right)
            }
            naga::Expression::Select {
                condition,
                accept,
                reject,
            } => {
                let (condition, accept, reject) = (*condition, *accept, *reject);
                let condition = self.eval(condition);
                let accept = self.eval(accept);
                let reject = self.eval(reject);
                match (condition, accept, reject) {
                    (Val::Bool(c), a, r) => {
                        if c {
                            a
                        } else {
                            r
                        }
                    }
                    (Val::Composite(c), Val::Composite(a), Val::Composite(r)) => Val::Composite(
                        c.into_iter()
                            .zip(a.into_iter().zip(r))
                            .map(|(c, (a, r))| if c == Val::Bool(true) { a } else { r })
                            .collect(),
                    ),
                    _ => Val::Unknown,
                }
            }
            naga::Expression::Math {
                fun,
                arg,
                arg1,
                arg2,
                ..
            } => {
                let (fun, arg, arg1, arg2) = (*fun, *arg, *arg1, *arg2);
                let mut args = vec![self.eval(arg)];
                args.extend(arg1.map(|a| self.eval(a)));
                args.extend(arg2.map(|a| self.eval(a)));
                match fun {
                    naga::MathFunction::Mix => {
                        componentwise(&args, &|o| o[0] + (o[1] - o[0]) * o[2])
                    }
                    naga::MathFunction::Min => componentwise(&args, &|o| o[0].min(o[1])),
                    naga::MathFunction::Max => componentwise(&args, &|o| o[0].max(o[1])),
                    naga::MathFunction::Clamp => {
                        componentwise(&args, &|o| o[0].max(o[1]).min(o[2]))
                    }
                    naga::MathFunction::Abs => componentwise(&args, &|o| o[0].abs()),
                    naga::MathFunction::Floor => componentwise(&args, &|o| o[0].floor()),
                    naga::MathFunction::Fract => componentwise(&args, &|o| o[0] - o[0].floor()),
                    _ => Val::Unknown,
                }
            }
            naga::Expression::As {
                expr,
                kind,
                convert,
            } => {
                let (kind, convert) = (*kind, convert.is_some());
                let value = self.eval(*expr);
                cast(value, kind, convert)
            }
            _ => Val::Unknown,
        };
        self.cache.insert(h, value.clone());
        value
    }

    fn exec(&mut self, block: &'m naga::Block) -> Flow {
        for statement in block.iter() {
            match statement {
                naga::Statement::Emit(range) => {
                    for h in range.clone() {
                        self.eval(h);
                    }
                }
                naga::Statement::Block(inner) => match self.exec(inner) {
                    Flow::Next => {}
                    other => return other,
                },
                naga::Statement::If {
                    condition,
                    accept,
                    reject,
                } => {
                    let taken = match self.eval(*condition) {
                        Val::Bool(true) => accept,
                        Val::Bool(false) => reject,
                        other => panic!(
                            "vs_main branches on a value the evaluator cannot model: {other:?}"
                        ),
                    };
                    match self.exec(taken) {
                        Flow::Next => {}
                        other => return other,
                    }
                }
                naga::Statement::Switch { selector, cases } => {
                    let selected = self.eval(*selector);
                    let start = cases
                        .iter()
                        .position(|case| match (&case.value, &selected) {
                            (naga::SwitchValue::U32(v), Val::U32(s)) => v == s,
                            (naga::SwitchValue::I32(v), Val::I32(s)) => v == s,
                            _ => false,
                        })
                        .or_else(|| {
                            cases
                                .iter()
                                .position(|case| case.value == naga::SwitchValue::Default)
                        });
                    if let Some(start) = start {
                        for case in &cases[start..] {
                            match self.exec(&case.body) {
                                Flow::Next if case.fall_through => continue,
                                Flow::Next | Flow::Break => break,
                                ret @ Flow::Return(_) => return ret,
                            }
                        }
                    }
                }
                naga::Statement::Break => return Flow::Break,
                naga::Statement::Return { value } => {
                    let value = value.map(|v| self.eval(v)).unwrap_or(Val::Unknown);
                    return Flow::Return(value);
                }
                naga::Statement::Store { pointer, value } => {
                    let (pointer, value) = (*pointer, *value);
                    let pointer = self.eval(pointer);
                    let value = self.eval(value);
                    self.write(pointer, value);
                }
                naga::Statement::Loop { .. } => {
                    panic!(
                        "vs_main loops; the evaluator models straight-line code and branches only"
                    )
                }
                naga::Statement::Call { .. } => {
                    panic!("vs_main calls a function; the evaluator does not follow calls")
                }
                _ => {}
            }
        }
        Flow::Next
    }
}

fn floats(value: &Val) -> Option<Vec<f32>> {
    match value {
        Val::Composite(items) => items
            .iter()
            .map(|v| match v {
                Val::F32(f) => Some(*f),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// The corner `vs_main` of the composed UI pipeline puts vertex `vertex_index`
/// of a unit quad on: read from its UV (`@location(0)`, with UVs spanning
/// [0, 1]) and checked against its clip-space position (instance at the origin
/// of size 1, identity projection), which must agree.
fn ui_corner(module: &naga::Module, vertex_index: u32) -> (f32, f32) {
    let entry = module
        .entry_points
        .iter()
        .find(|e| e.stage == naga::ShaderStage::Vertex && e.name == "vs_main")
        .expect("vs_main vertex entry point");
    let result = entry
        .function
        .result
        .as_ref()
        .expect("vs_main returns its vertex");
    let naga::TypeInner::Struct { members, .. } = &module.types[result.ty].inner else {
        panic!("vs_main returns a struct of varyings");
    };
    let member = |wanted: &dyn Fn(&naga::Binding) -> bool| {
        members
            .iter()
            .position(|m| m.binding.as_ref().is_some_and(wanted))
    };
    let uv_at = member(&|b| matches!(b, naga::Binding::Location { location: 0, .. }))
        .expect("vs_main outputs the quad's UV at location 0");
    let position_at =
        member(&|b| matches!(b, naga::Binding::BuiltIn(naga::BuiltIn::Position { .. })))
            .expect("vs_main outputs a clip position");

    let mut eval = VertexEval::new(module, entry, vertex_index);
    let Flow::Return(Val::Composite(out)) = eval.exec(&entry.function.body) else {
        panic!("vertex {vertex_index}: vs_main returned no value the evaluator could read");
    };
    let uv = floats(&out[uv_at])
        .unwrap_or_else(|| panic!("vertex {vertex_index}: the UV is not a float vector"));
    let clip = floats(&out[position_at]).unwrap_or_else(|| {
        panic!("vertex {vertex_index}: the clip position is not a float vector")
    });
    assert_eq!(
        (clip[0], clip[1]),
        (uv[0], uv[1]),
        "vertex {vertex_index}: the corner the quad is drawn at differs from the corner it samples"
    );
    (uv[0], uv[1])
}

/// The UI lane draws each quad as a four-vertex triangle strip: vertex 0..3
/// must be (0,0), (1,0), (0,1), (1,1), so that triangles (0,1,2) and (1,2,3)
/// cover the quad with no gap and no overlap.
#[test]
fn ui_shader_corners_follow_strip_order() {
    let module = compose_pipeline("khora::pipelines::ui");

    let corners: Vec<(f32, f32)> = (0..4).map(|v| ui_corner(&module, v)).collect();

    assert_eq!(
        corners,
        [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)],
        "the UI quad's vertices 0..3 must follow triangle-strip order"
    );
}
