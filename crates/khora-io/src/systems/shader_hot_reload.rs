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
//! [`AssetWatch`] each tick, maps every changed `.wgsl` file to its
//! logical shader name, hands the new source to the
//! [`PipelineSystem`](khora_core::renderer::traits::PipelineSystem) backend as
//! an overlay, and recomposes the affected cached pipelines in place. Lanes
//! re-fetch pipelines by key every frame, so they pick up the rebuilt pipeline
//! with no lane-side change.
//!
//! With no assets directory present, no watcher is registered and the backend
//! keeps serving its embedded `include_str!` sources — the production path.

use khora_data::ecs::SystemContext;
use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_core::renderer::traits::{GraphicsDevice, PipelineSystem};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};

use crate::asset::{AssetChangeEvent, AssetChangeKind, AssetWatch};

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

/// The source of the overlay `rel_path` names under the watched `root`,
/// read through a [`FileGuard`](crate::trust::FileGuard) that reports its
/// refusals to `refusals`; `None` when it is refused or cannot be read.
pub fn overlay_source(
    root: &std::path::Path,
    rel_path: &str,
    refusals: &Arc<crate::trust::Refusals>,
) -> Option<String> {
    let guard =
        crate::trust::FileGuard::new(root, crate::trust::Trust::Project, Arc::clone(refusals));
    match guard.read_to_string(rel_path) {
        Ok(source) => Some(source),
        Err(error) => {
            log::warn!(
                "shader hot-reload: {} is not read: {error}",
                root.join(rel_path).display()
            );
            None
        }
    }
}

/// The files `event` says changed, as their root and their path under it: a
/// rescan names every file under its root.
fn changed_files(event: &AssetChangeEvent) -> Vec<(std::path::PathBuf, String)> {
    let root = event.root().to_path_buf();
    if event.kind != AssetChangeKind::Rescan {
        return vec![(root, event.rel_path.clone())];
    }
    walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let rel = entry.path().strip_prefix(&root).ok()?;
            Some((root.clone(), khora_core::asset::asset_key(rel)))
        })
        .collect()
}

/// Lists the overlay at `file` among what the restricted mode left unused,
/// under [`ProblemSource::Restricted`] — each file once; the list is cleared
/// when the project is trusted.
fn list_ignored(runtime: &Runtime, file: &std::path::Path) {
    let Some(problems) = runtime.resources.get::<Arc<Problems>>() else {
        return;
    };
    let module = file.display().to_string();
    let mut listed = problems.of_source(&ProblemSource::Restricted);
    if listed
        .iter()
        .any(|problem| problem.module.as_deref() == Some(&module))
    {
        return;
    }
    listed.push(Problem {
        severity: ProblemSeverity::Warning,
        module: Some(module),
        line: 0,
        column: 0,
        message: "Shader overlay ignored".to_owned(),
        note: Some(
            "A restricted project's shaders are not used. Trust the project to use them."
                .to_owned(),
        ),
        entity: None,
        behavior: None,
        component: None,
        field: None,
    });
    problems.report(ProblemSource::Restricted, listed);
}

fn shader_hot_reload_system(
    _world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(watch) = runtime.resources.get::<Arc<AssetWatch>>() else {
        return; // No assets dir → embedded sources, nothing to pump.
    };
    let events = watch.poll_for("shader_hot_reload");
    if events.is_empty() {
        return;
    }
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };

    let refusals = runtime
        .resources
        .get::<Arc<crate::trust::Refusals>>()
        .cloned()
        .unwrap_or_else(|| Arc::new(crate::trust::Refusals::new(None, None)));
    // A restricted project's shaders never replace the engine's.
    let restricted =
        crate::trust::ProjectTrustState::of(runtime) == crate::trust::ProjectTrust::Restricted;
    let mut changed = false;
    for (root, rel_path) in events.iter().flat_map(changed_files) {
        let Some(logical) = logical_name_for_shader_path(&rel_path) else {
            continue; // Not a shader file.
        };
        if restricted {
            log::debug!(
                "shader hot-reload: {rel_path} is not used for `{logical}`: the project is restricted"
            );
            list_ignored(runtime, &root.join(&rel_path));
            continue;
        }
        if let Some(source) = overlay_source(&root, &rel_path, &refusals) {
            log::info!("shader hot-reload: {rel_path} replaces the engine's `{logical}`");
            pipeline_system.set_overlay_source(&logical, source);
            changed = true;
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
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
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
