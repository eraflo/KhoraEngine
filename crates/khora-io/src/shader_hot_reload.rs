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

//! `.wgsl` hot-reload pump.
//!
//! A `PreExtract` data system that drains the shader
//! [`AssetWatcher`] each tick, maps every changed `.wgsl` file to its
//! logical shader name, hands the new source to the
//! [`PipelineSystem`](khora_core::renderer::traits::PipelineSystem) backend as
//! an overlay, and recomposes the affected cached pipelines in place. Lanes
//! re-fetch pipelines by key every frame, so they pick up the rebuilt pipeline
//! with no lane-side change.
//!
//! With no assets directory present, no watcher is registered and the backend
//! keeps serving its embedded `include_str!` sources — the production path.

use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::renderer::traits::{GraphicsDevice, PipelineSystem};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};

use crate::asset::AssetWatcher;

/// Root segment every composable shader module path carries.
const LOGICAL_ROOT: &str = "khora";

/// Maps a watcher-relative `.wgsl` path to its logical shader module name.
///
/// The shader tree mirrors the logical namespace: `shaders/lib/std/camera.wgsl`
/// ↔ `khora::std::camera`, `shaders/pipelines/standard_pbr.wgsl` ↔
/// `khora::pipelines::standard_pbr`. The mapping strips the `shaders/` root and
/// the `lib/` grouping segment (libs are namespaced directly under `khora::`),
/// drops the `.wgsl` extension, replaces `/` with `::`, and prefixes `khora::`.
/// Returns `None` for paths that are not `.wgsl` under `shaders/`.
pub fn logical_name_for_shader_path(rel_path: &str) -> Option<String> {
    let rel = rel_path.replace('\\', "/");
    let stem = rel.strip_suffix(".wgsl")?;
    // Locate the `shaders/` root anywhere in the path (the watcher root may be
    // the assets dir, so the prefix is `.../shaders/...`).
    let after_root = stem.split("shaders/").last()?;
    // Group segment `lib/` is implicit in the logical namespace; pipelines keep
    // their `pipelines/` segment.
    let body = after_root.strip_prefix("lib/").unwrap_or(after_root);
    if body.is_empty() {
        return None;
    }
    let namespaced = body.replace('/', "::");
    Some(format!("{LOGICAL_ROOT}::{namespaced}"))
}

fn shader_hot_reload_system(_world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(watcher) = runtime.resources.get::<Arc<AssetWatcher>>() else {
        return; // No assets dir → embedded sources, nothing to pump.
    };
    let events = watcher.poll();
    if events.is_empty() {
        return;
    }
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };

    let assets_root = watcher.assets_root();
    let mut changed = false;
    for event in events {
        let Some(logical) = logical_name_for_shader_path(&event.rel_path) else {
            continue; // Not a shader file.
        };
        let abs = assets_root.join(&event.rel_path);
        match std::fs::read_to_string(&abs) {
            Ok(source) => {
                pipeline_system.set_overlay_source(&logical, source);
                changed = true;
            }
            Err(e) => {
                log::warn!(
                    "shader hot-reload: failed to read {} for `{logical}`: {e}",
                    abs.display()
                );
            }
        }
    }

    if changed {
        if let Err(e) = pipeline_system.recompose_dirty(device.as_ref()) {
            log::error!("shader hot-reload: recompose failed: {e}");
        }
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "shader_hot_reload",
        phase: TickPhase::PreExtract,
        run: shader_hot_reload_system,
        order_hint: -10,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_lib_path_to_logical() {
        assert_eq!(
            logical_name_for_shader_path("shaders/lib/std/camera.wgsl").as_deref(),
            Some("khora::std::camera")
        );
        assert_eq!(
            logical_name_for_shader_path("shaders/lib/std/material_textures.wgsl").as_deref(),
            Some("khora::std::material_textures")
        );
        assert_eq!(
            logical_name_for_shader_path("shaders/lib/shadow/sample_cube.wgsl").as_deref(),
            Some("khora::shadow::sample_cube")
        );
    }

    #[test]
    fn maps_pipeline_path_to_logical() {
        assert_eq!(
            logical_name_for_shader_path("shaders/pipelines/standard_pbr.wgsl").as_deref(),
            Some("khora::pipelines::standard_pbr")
        );
        assert_eq!(
            logical_name_for_shader_path("shaders/pipelines/light_culling.wgsl").as_deref(),
            Some("khora::pipelines::light_culling")
        );
    }

    #[test]
    fn handles_nested_root_and_backslashes() {
        assert_eq!(
            logical_name_for_shader_path("assets\\shaders\\lib\\lighting\\structs.wgsl").as_deref(),
            Some("khora::lighting::structs")
        );
    }

    #[test]
    fn rejects_non_shader_paths() {
        assert_eq!(logical_name_for_shader_path("textures/foo.png"), None);
        assert_eq!(
            logical_name_for_shader_path("shaders/lib/std/camera.txt"),
            None
        );
    }
}
