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

//! Which pipelines depend on which shader modules, from their `#import`s.

use std::collections::HashSet;

use khora_core::renderer::api::pipeline::{ComputePipelineKey, PipelineKey};
use khora_core::renderer::error::RenderError;

use super::rebuild::inject_float_defs;
use super::{backend_err, compose_err, Inner, LIB_MODULES};
use naga_oil::compose::{ComposableModuleDescriptor, ShaderLanguage};

/// Re-registers a dirty lib module with the composer using its overlay (or
/// embedded) source, replacing the previous registration. `naga_oil` allows
/// re-adding a composable module under the same import path.
///
/// A watched file the lib table does not know and that declares no
/// `#define_import_path` is not a lib module but a whole shader (the
/// `standalone/` ones, handed to their consumers as raw strings): there is
/// nothing to re-register, so it is skipped rather than failed.
pub(super) fn readd_lib_module(inner: &mut Inner, path: &str) -> Result<(), RenderError> {
    let source = resolve_lib_source(inner, path)
        .ok_or_else(|| backend_err(&format!("lib module `{path}` has no source")))?;
    let known_lib = LIB_MODULES.iter().any(|(p, _)| *p == path);
    if !known_lib && !source.contains("#define_import_path") {
        log::debug!("shader hot-reload: `{path}` is not a lib module, nothing to recompose");
        return Ok(());
    }
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
pub(super) fn resolve_lib_source(inner: &Inner, path: &str) -> Option<String> {
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
pub(super) fn resolve_pipeline_source(inner: &Inner, name: &str) -> Option<String> {
    if let Some(src) = inner.overlays.get(name) {
        return Some(src.clone());
    }
    inner.pipelines.get(name).map(|s| (*s).to_string())
}

/// Cached render-pipeline keys whose module graph transitively includes any of
/// the `dirty` logical names (the pipeline's own module or any imported lib).
pub(super) fn affected_render_pipelines(inner: &Inner, dirty: &[String]) -> Vec<PipelineKey> {
    let dirty_set: HashSet<&str> = dirty.iter().map(String::as_str).collect();
    inner
        .pipeline_specs
        .keys()
        .filter(|key| pipeline_depends_on(inner, key.shader, &dirty_set))
        .cloned()
        .collect()
}

/// Cached compute-pipeline keys affected by any `dirty` logical name.
pub(super) fn affected_compute_pipelines(
    inner: &Inner,
    dirty: &[String],
) -> Vec<ComputePipelineKey> {
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
pub(super) fn pipeline_depends_on(inner: &Inner, name: &str, dirty: &HashSet<&str>) -> bool {
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
pub(super) fn parse_imports(source: &str) -> Vec<String> {
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
