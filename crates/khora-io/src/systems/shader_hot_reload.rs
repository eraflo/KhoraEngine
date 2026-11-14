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

//! The `.wgsl` hot-reload pump: a project's changed shaders handed to the
//! [`PipelineSystem`] as overlays, never a restricted project's.

use khora_data::ecs::SystemContext;
use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_core::renderer::traits::{GraphicsDevice, PipelineSystem};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};

use crate::asset::{AssetChangeEvent, AssetChangeKind, AssetWatch};

/// What the shader pump last applied the project's overlays for: the watched roots and their trust.
///
/// A runtime resource, held as `Arc<OverlaysSeen>`.
#[derive(Default)]
pub struct OverlaysSeen {
    /// The watched roots and the trust last applied for; `None` before the
    /// first run. A leaf lock: held only to compare and replace.
    seen: std::sync::Mutex<Option<Vec<WatchedRoot>>>,
}

/// A watched root and the trust it is read at.
type WatchedRoot = (std::path::PathBuf, crate::trust::ProjectTrust);

impl OverlaysSeen {
    /// Records `roots`, each with its trust, and returns whether they differ from the last.
    fn moved(&self, roots: Vec<WatchedRoot>) -> bool {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Some(roots);
        if *seen == now {
            return false;
        }
        *seen = now;
        true
    }
}

/// Root segment every composable shader module path carries.
const LOGICAL_ROOT: &str = "khora";

/// Maps a watcher-relative `.wgsl` path to its logical shader module name.
///
/// `shaders/lib/std/camera.wgsl` is `khora::std::camera`,
/// `shaders/pipelines/lit_forward.wgsl` is `khora::pipelines::lit_forward`.
/// `None` for a path that is not `.wgsl` under `shaders/`.
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

/// Returns the source of the overlay `rel_path` names under the watched `root`.
///
/// Read through a [`FileGuard`](crate::trust::FileGuard) reporting to
/// `refusals`; `None` when it is refused or cannot be read.
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

/// Returns the files `event` says changed, as root and path: a rescan names every file.
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

/// Returns the warning listed for the overlay at `file`, left unused by a restricted project.
fn ignored(file: &std::path::Path) -> Problem {
    Problem {
        severity: ProblemSeverity::Warning,
        module: Some(file.display().to_string()),
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
    }
}

/// Lists the overlay at `file`, once, under [`ProblemSource::Restricted`].
fn list_ignored(runtime: &Runtime, file: &std::path::Path) {
    let Some(problems) = runtime.resources.get::<Arc<Problems>>() else {
        return;
    };
    let problem = ignored(file);
    let mut listed = problems.of_source(&ProblemSource::Restricted);
    if listed.iter().any(|known| known.module == problem.module) {
        return;
    }
    listed.push(problem);
    problems.report(ProblemSource::Restricted, listed);
}

/// Returns every overlay under `root`, as the absolute root and the path under it.
fn overlays_under(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let root = crate::asset::root_path::absolute(root);
    walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            let rel = entry.path().strip_prefix(&root).ok()?;
            let rel = khora_core::asset::asset_key(rel);
            logical_name_for_shader_path(&rel)?;
            Some((root.clone(), rel))
        })
        .collect()
}

/// Applies the overlays of the watched `roots` afresh, each root at its trust.
///
/// A trusted root's overlays replace their modules; a restricted root's are
/// listed under [`ProblemSource::Restricted`], replacing the list.
fn apply_afresh(
    runtime: &Runtime,
    pipeline_system: &dyn PipelineSystem,
    roots: &[WatchedRoot],
    refusals: &Arc<crate::trust::Refusals>,
) {
    pipeline_system.clear_overlays();
    let mut listed = Vec::new();
    for (watched, trust) in roots {
        let restricted = *trust == crate::trust::ProjectTrust::Restricted;
        for (root, rel_path) in overlays_under(watched) {
            let Some(logical) = logical_name_for_shader_path(&rel_path) else {
                continue;
            };
            if restricted {
                listed.push(ignored(&root.join(&rel_path)));
            } else if let Some(source) = overlay_source(&root, &rel_path, refusals) {
                log::info!("shader hot-reload: {rel_path} replaces the engine's `{logical}`");
                pipeline_system.set_overlay_source(&logical, source);
            }
        }
    }
    if let Some(problems) = runtime.resources.get::<Arc<Problems>>() {
        problems.report(ProblemSource::Restricted, listed);
    }
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
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    // A root outside the project open, such as the previous one still
    // watched, is read restricted.
    let roots: Vec<WatchedRoot> = (watch.roots().into_iter())
        .map(|root| {
            let trust = crate::trust::ProjectTrustState::of_path(runtime, &root);
            (root, trust)
        })
        .collect();
    // A project opened, closed, trusted or restricted: its overlays are
    // applied afresh, those on disk included — not only the ones saved since.
    let moved = runtime
        .resources
        .get::<Arc<OverlaysSeen>>()
        .is_some_and(|seen| seen.moved(roots.clone()));
    let events = watch.poll_for("shader_hot_reload");
    if events.is_empty() && !moved {
        return;
    }

    let refusals = runtime
        .resources
        .get::<Arc<crate::trust::Refusals>>()
        .cloned()
        .unwrap_or_else(|| Arc::new(crate::trust::Refusals::new(None, None)));
    // An overlay deleted gives its module back, so the overlays are applied
    // afresh: a shader or a folder (no extension) removed, or a rescan, which
    // may have dropped a removal.
    let removed = events.iter().any(|event| match event.kind {
        AssetChangeKind::Rescan => true,
        AssetChangeKind::Removed => {
            logical_name_for_shader_path(&event.rel_path).is_some()
                || std::path::Path::new(&event.rel_path).extension().is_none()
        }
        _ => false,
    });
    let mut changed = moved || removed;
    if changed {
        apply_afresh(runtime, pipeline_system.as_ref(), &roots, &refusals);
    }
    // A deleted file has nothing to read: the afresh pass above handled it.
    let written = events
        .iter()
        .filter(|event| event.kind != AssetChangeKind::Removed);
    for (root, rel_path) in written.flat_map(changed_files) {
        let Some(logical) = logical_name_for_shader_path(&rel_path) else {
            continue; // Not a shader file.
        };
        // A restricted project's shaders never replace the engine's.
        let restricted = crate::trust::ProjectTrustState::of_path(runtime, &root)
            == crate::trust::ProjectTrust::Restricted;
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
            logical_name_for_shader_path("shaders/pipelines/lit_forward.wgsl").as_deref(),
            Some("khora::pipelines::lit_forward")
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
