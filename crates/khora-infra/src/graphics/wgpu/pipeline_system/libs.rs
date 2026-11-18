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

//! The lib modules the composer holds: built each after its imports, and
//! recomposed one at a time, all or nothing.

use std::collections::HashMap;

use khora_core::renderer::error::RenderError;
use naga_oil::compose::{ComposableModuleDescriptor, Composer, ShaderDefValue, ShaderLanguage};

use super::dependencies::resolve_lib_source;
use super::rebuild::inject_float_defs;
use super::{backend_err, compose_err, Inner, LIB_MODULES};

/// Builds a composer holding every lib of `sources`, each added after the libs it imports.
///
/// Each lib is added once, so none is dropped as the dependent of a lib added again.
///
/// # Errors
///
/// Fails when a lib does not compose (no `#define_import_path`, an import
/// no lib of `sources` declares), or when a lib is missing from the composer
/// once all are added: a source declares another lib's import path.
pub(super) fn compose_libs(
    sources: &[(&str, &str)],
    base_defs: &HashMap<String, ShaderDefValue>,
) -> Result<Composer, RenderError> {
    let mut composer = Composer::default();
    let mut pending: Vec<(&str, &str)> = sources.to_vec();
    // A lib whose imports are not added yet fails; it is tried again once
    // others are, until a pass adds none.
    while !pending.is_empty() {
        let before = pending.len();
        let mut first_error = None;
        pending.retain(
            |(path, source)| match add_lib(&mut composer, path, source, base_defs) {
                Ok(()) => false,
                Err(error) => {
                    first_error.get_or_insert(error);
                    true
                }
            },
        );
        match first_error {
            Some(error) if pending.len() == before => return Err(error),
            _ => {}
        }
    }
    if let Some((missing, _)) = sources
        .iter()
        .find(|(path, _)| !composer.contains_module(path))
    {
        return Err(backend_err(&format!(
            "lib `{missing}` is not composed: a source declares its import path in place of its own"
        )));
    }
    Ok(composer)
}

/// Recomposes the registered lib `path` from its current source, all or nothing.
///
/// On success the composer holds every registered lib, `path` from its
/// current source. A source the composer already holds recomposes nothing.
///
/// # Errors
///
/// Fails, leaving the composer and the sources it holds as they were, when
/// `path` has no source or the libs do not compose with its current one
/// (see [`compose_libs`]).
pub(super) fn recompose_lib(inner: &mut Inner, path: &'static str) -> Result<(), RenderError> {
    let source = resolve_lib_source(inner, path)
        .ok_or_else(|| backend_err(&format!("lib module `{path}` has no source")))?;
    if inner.composed_libs.get(path) == Some(&source) {
        return Ok(());
    }
    let sources: Vec<(&str, &str)> = LIB_MODULES
        .iter()
        .map(|(lib, embedded)| {
            let held = if *lib == path {
                source.as_str()
            } else {
                inner
                    .composed_libs
                    .get(lib)
                    .map_or(*embedded, String::as_str)
            };
            (*lib, held)
        })
        .collect();
    let composer = compose_libs(&sources, &inner.base_defs)?;
    inner.composer = composer;
    inner.composed_libs.insert(path, source);
    Ok(())
}

/// Returns the engine's own source of every registered lib, by import path.
pub(super) fn engine_lib_sources() -> HashMap<&'static str, String> {
    LIB_MODULES
        .iter()
        .map(|(path, source)| (*path, (*source).to_owned()))
        .collect()
}

/// Adds the lib `path` to `composer` from `source`, with the float constants and `base_defs`.
fn add_lib(
    composer: &mut Composer,
    path: &str,
    source: &str,
    base_defs: &HashMap<String, ShaderDefValue>,
) -> Result<(), RenderError> {
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
    Ok(())
}
