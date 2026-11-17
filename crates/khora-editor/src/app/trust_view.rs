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

//! What the editor shows of the trust of the project open.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::EditorRequest;
use khora_sdk::editor_ui::{EditorShell, PanelLocation, QuarantinedAsset, TrustSummary};
use khora_sdk::fs;
use khora_sdk::khora_core::asset::asset_key;
use khora_sdk::trust::{ProjectTrustState, QuarantineKey, Refusals, SecurityRefusal, Trust};
use khora_sdk::Runtime;

use crate::chrome::RestrictedBarPanel;

/// What the editor last showed of the project's trust.
#[derive(Default)]
pub(super) struct TrustView {
    /// Whether the restricted bar is on the shell.
    bar_shown: bool,
    /// What the quarantine was last mirrored at.
    quarantine_seen: Option<Seen>,
}

/// The moment the quarantine was mirrored at: it is mirrored again once either moves.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    /// The problems' generation.
    generation: u64,
    /// The project folder open.
    project: Option<String>,
}

impl super::EditorApp {
    /// Brings what the editor shows of the project's trust in line with the engine, each frame.
    ///
    /// Called after [`follow_trust_answers`](super::EditorApp::follow_trust_answers).
    /// Mirrors `ProjectTrustState` into `EditorState::restricted` and the run's
    /// `Refusals` into `EditorState::quarantined`, shows the restricted bar while
    /// restricted, answers `ConfirmTrust` with what trusting would change, and
    /// releases the assets each `EditorRequest::ReleaseAsset` names. See the book:
    /// reference/editor.md, "11 — Restricted projects".
    pub(super) fn follow_project_trust(&mut self, runtime: &Runtime) {
        let restricted = ProjectTrustState::is_restricted(runtime);
        let refusals = runtime.resources.get::<Arc<Refusals>>().cloned();
        let generation = runtime
            .resources
            .get::<Arc<khora_sdk::khora_core::problems::Problems>>()
            .map(|problems| problems.generation());

        let (confirm, releases, project) = match self.editor_state.lock() {
            Ok(mut state) => {
                state.restricted = restricted;
                let confirm = !(state.take_requests(|request| match request {
                    EditorRequest::ConfirmTrust => Ok(()),
                    other => Err(other),
                }))
                .is_empty();
                let releases = state.take_requests(|request| match request {
                    EditorRequest::ReleaseAsset { path } => Ok(path),
                    other => Err(other),
                });
                (confirm, releases, state.project_folder.clone())
            }
            Err(_) => return,
        };

        if restricted != self.trust_view.bar_shown {
            self.show_restricted_bar(runtime, restricted);
        }

        let root = project.as_deref().map(PathBuf::from);
        let assets = root.as_ref().map(|root| root.join("assets"));

        if let (Some(assets), Some(refusals)) = (&assets, &refusals) {
            for rel in releases {
                match under_assets(&rel) {
                    Some(rel_path) => {
                        refusals.release(&assets.join(&rel_path));
                        self.read_again(runtime, &rel_path);
                        log::info!("'{rel}' is released from the quarantine: it is read again");
                    }
                    None => {
                        log::warn!("'{rel}' names no file of the project: nothing is released")
                    }
                }
            }
        }

        if confirm {
            if let Some(root) = &root {
                let summary = trust_summary(root, refusals.as_deref());
                if let Ok(mut state) = self.editor_state.lock() {
                    state.trust_confirm = Some(summary);
                }
            }
        }

        let seen = generation.map(|generation| Seen {
            generation,
            project: project.clone(),
        });
        if seen.is_some() && seen != self.trust_view.quarantine_seen {
            self.trust_view.quarantine_seen = seen;
            let quarantined = match (&assets, &refusals) {
                (Some(assets), Some(refusals)) => (refusals.quarantine().into_iter())
                    .filter_map(|(refusal, key)| shown(assets, &refusal, &key))
                    .collect(),
                _ => Vec::new(),
            };
            if let Ok(mut state) = self.editor_state.lock() {
                state.quarantined = quarantined;
            }
        }
    }

    /// Forgets what the asset services remember of the file at `rel_path`, a
    /// load the quarantine failed, so its next load reads it.
    fn read_again(&self, runtime: &Runtime, rel_path: &Path) {
        let uuid = khora_sdk::khora_core::asset::AssetUUID::new_v5(
            &khora_sdk::khora_core::asset::asset_key(rel_path),
        );
        let engine = (runtime.services)
            .get::<Arc<Mutex<khora_sdk::AssetService>>>()
            .cloned();
        for service in engine.iter().chain([&self.shared_assets]) {
            if let Ok(mut service) = service.lock() {
                service.invalidate(&uuid);
            }
        }
    }

    /// Puts the restricted bar on the shell, or takes it off.
    fn show_restricted_bar(&mut self, runtime: &Runtime, show: bool) {
        let Some(shell) = runtime
            .backends
            .get::<Arc<Mutex<Box<dyn EditorShell>>>>()
            .cloned()
        else {
            return;
        };
        let Ok(mut shell) = shell.lock() else {
            return;
        };
        if show {
            shell.register_panel(
                PanelLocation::TopBar,
                Box::new(RestrictedBarPanel::new(
                    Arc::clone(&self.editor_state),
                    khora_tool_ui::khora_dark(),
                )),
            );
        } else {
            shell.remove_panel(RestrictedBarPanel::ID);
        }
        self.trust_view.bar_shown = show;
    }
}

/// What trusting the project at `root` changes, read from its folder and
/// from what `refusals` holds of it.
fn trust_summary(root: &Path, refusals: Option<&Refusals>) -> TrustSummary {
    let assets = root.join("assets");
    let scripts = with_extension(&assets.join("scripts"), "erg").count();
    // The shader reload takes any `.wgsl` under the assets as an overlay of
    // the module its path names.
    let mut overlays: Vec<String> = with_extension(&assets, "wgsl")
        .filter_map(|rel| khora_sdk::logical_name_for_shader_path(&asset_key(&rel)))
        .collect();
    overlays.sort();
    overlays.dedup();
    let refused_for_size = refusals.map_or(0, |refusals| {
        (refusals.quarantine().iter())
            .filter(|(refusal, _)| {
                refusal.trust == Trust::Restricted
                    && refusal.reason.is_size_or_limit()
                    && fs::is_under(&refusal.path, root)
            })
            .count()
    });
    TrustSummary {
        scripts,
        overlays,
        builds_code: root.join("Cargo.toml").is_file(),
        refused_for_size,
    }
}

/// `rel`, a forward-slash path the asset browser gave, as a path under the
/// assets; `None` when it would leave them — a `..`, a root, a drive.
fn under_assets(rel: &str) -> Option<PathBuf> {
    if rel.starts_with('/') {
        return None;
    }
    let mut path = PathBuf::new();
    for part in rel.split('/').filter(|part| !part.is_empty()) {
        let mut components = Path::new(part).components();
        match (components.next(), components.next()) {
            (Some(std::path::Component::Normal(name)), None) => path.push(name),
            _ => return None,
        }
    }
    (!path.as_os_str().is_empty()).then_some(path)
}

/// The files under `dir`, at any depth, whose extension is `extension`, by their path under `dir`.
fn with_extension<'a>(dir: &Path, extension: &'a str) -> impl Iterator<Item = PathBuf> + 'a {
    (fs::files_under(dir).into_iter())
        .filter(move |rel| rel.extension().and_then(|e| e.to_str()) == Some(extension))
}

/// What the asset browser shows of `refusal`, a file of the project whose
/// assets are at `assets`; `None` for another project's file.
fn shown(
    assets: &Path,
    refusal: &SecurityRefusal,
    key: &QuarantineKey,
) -> Option<QuarantinedAsset> {
    let rel_path = asset_key(&fs::relative_to(&refusal.path, assets)?);
    Some(QuarantinedAsset {
        rel_path,
        reason: refusal.reason.to_string(),
        remedy: refusal.reason.remedy().to_owned(),
        refused_by: refusal.refused_by().to_owned(),
        read_as: refusal.trust.label().to_owned(),
        content: key.short_label(),
    })
}
