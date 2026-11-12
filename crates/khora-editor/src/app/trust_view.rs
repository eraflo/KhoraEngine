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

use khora_sdk::editor_ui::{EditorShell, PanelLocation, QuarantinedAsset, TrustSummary};
use khora_sdk::trust::{
    Layer, ProjectTrust, ProjectTrustState, QuarantineKey, Refusals, SecurityReason,
    SecurityRefusal, Trust,
};
use khora_sdk::Runtime;

use crate::chrome::RestrictedBarPanel;

/// What the editor last showed of the project's trust.
#[derive(Default)]
pub(super) struct TrustView {
    /// Whether the restricted bar is on the shell.
    bar_shown: bool,
    /// The problems' generation and the project folder the quarantine was
    /// last mirrored at.
    quarantine_seen: Option<(u64, Option<String>)>,
}

impl super::EditorApp {
    /// Brings what the editor shows of the project's trust in line with the
    /// engine, each frame (`before_frame`, after
    /// [`follow_trust_answers`](super::EditorApp::follow_trust_answers)):
    /// - `EditorState::restricted` mirrors the runtime's `ProjectTrustState`;
    /// - the restricted bar (`RestrictedBarPanel`, `TopBar`) is registered
    ///   with the shell when the project turns restricted, removed when it
    ///   turns trusted — never twice;
    /// - the `confirm_trust` menu action (the restricted bar's `Trust this
    ///   project…`) is taken and fills `EditorState::trust_confirm` with what
    ///   trusting the project open changes, read from its folder: the `.erg`
    ///   files under `assets/scripts`, the `.wgsl` under `assets/shaders` by
    ///   their logical names, a `Cargo.toml` at its root, its files the
    ///   run's `Refusals` quarantined at `Trust::Restricted` for a size or a
    ///   limit;
    /// - `EditorState::quarantined` mirrors the run's `Refusals` (the paths
    ///   relative to the project's assets root) when the problems' generation
    ///   moves;
    /// - `EditorState::pending_release` is taken and its file released from
    ///   the quarantine.
    pub(super) fn follow_project_trust(&mut self, runtime: &Runtime) {
        let restricted = ProjectTrustState::of(runtime) == ProjectTrust::Restricted;
        let refusals = runtime.resources.get::<Arc<Refusals>>().cloned();
        let generation = runtime
            .resources
            .get::<Arc<khora_sdk::khora_core::problems::Problems>>()
            .map(|problems| problems.generation());

        let (confirm, release, project) = match self.editor_state.lock() {
            Ok(mut state) => {
                state.restricted = restricted;
                let confirm = state.pending_menu_action.as_deref() == Some("confirm_trust");
                if confirm {
                    state.pending_menu_action = None;
                }
                (
                    confirm,
                    state.pending_release.take(),
                    state.project_folder.clone(),
                )
            }
            Err(_) => return,
        };

        if restricted != self.trust_view.bar_shown {
            self.show_restricted_bar(runtime, restricted);
        }

        let root = project.as_deref().map(PathBuf::from);
        let assets = root.as_ref().map(|root| root.join("assets"));

        if let (Some(rel), Some(assets), Some(refusals)) = (release, &assets, &refusals) {
            match under_assets(&rel) {
                Some(rel_path) => {
                    refusals.release(&assets.join(&rel_path));
                    self.read_again(runtime, &rel_path);
                    log::info!("'{rel}' is released from the quarantine: it is read again");
                }
                None => log::warn!("'{rel}' names no file of the project: nothing is released"),
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

        let seen = generation.map(|generation| (generation, project.clone()));
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

    /// Forgets what the asset services remember of the file at `rel_path`
    /// under the assets — a load the quarantine failed — so its next load
    /// reads it.
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
    let scripts = files_under(&assets.join("scripts"), "erg").len();
    // The shader reload takes any `.wgsl` under the assets as an overlay of
    // the module its path names.
    let mut overlays: Vec<String> = files_under(&assets, "wgsl")
        .iter()
        .filter_map(|path| {
            let rel = path.strip_prefix(&assets).ok()?;
            khora_sdk::logical_name_for_shader_path(&rel.to_string_lossy())
        })
        .collect();
    overlays.sort();
    overlays.dedup();
    let refused_for_size = refusals.map_or(0, |refusals| {
        (refusals.quarantine().iter())
            .filter(|(refusal, _)| {
                refusal.trust == Trust::Restricted
                    && matches!(
                        refusal.reason,
                        SecurityReason::TooLarge { .. } | SecurityReason::OverLimit { .. }
                    )
                    && under(&refusal.path, root)
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

/// The files under `dir`, at any depth, whose extension is `extension`.
fn files_under(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                // A link is not followed: it could lead out of the project.
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind)
                    if kind.is_file()
                        && path.extension().and_then(|e| e.to_str()) == Some(extension) =>
                {
                    found.push(path);
                }
                _ => {}
            }
        }
    }
    found
}

/// `path` relative to `root`, as given or once both are resolved.
fn relative(path: &Path, root: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    let (path, root) = (path.canonicalize().ok()?, root.canonicalize().ok()?);
    path.strip_prefix(root).ok().map(Path::to_path_buf)
}

/// Whether `path` is under `root`.
fn under(path: &Path, root: &Path) -> bool {
    relative(path, root).is_some()
}

/// What the asset browser shows of `refusal`, a file of the project whose
/// assets are at `assets`; `None` for another project's file.
fn shown(
    assets: &Path,
    refusal: &SecurityRefusal,
    key: &QuarantineKey,
) -> Option<QuarantinedAsset> {
    let rel = relative(&refusal.path, assets)?;
    let rel_path = (rel.components())
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Some(QuarantinedAsset {
        rel_path,
        reason: refusal.reason.to_string(),
        remedy: refusal.reason.remedy().to_owned(),
        refused_by: refused_by(refusal).to_owned(),
        read_as: read_as(refusal.trust).to_owned(),
        content: held_by(key),
    })
}

/// What refused the file, in the browser's words.
fn refused_by(refusal: &SecurityRefusal) -> &'static str {
    if matches!(refusal.reason, SecurityReason::WrongType { .. }) {
        return "type check";
    }
    match refusal.layer {
        Layer::File => "file guard",
        Layer::Decoder => "decoder",
        Layer::Content => "type check",
        Layer::Pack => "pack",
    }
}

/// The trust a file was read at, in the browser's words.
fn read_as(trust: Trust) -> &'static str {
    match trust {
        Trust::Engine => "engine",
        Trust::Project => "trusted project",
        Trust::Restricted => "restricted project",
        Trust::Imported => "downloaded file",
        Trust::Save => "save",
    }
}

/// What the quarantine holds the file by: its content's hash, shortened, or
/// its size and date.
fn held_by(key: &QuarantineKey) -> String {
    match key {
        QuarantineKey::Content(hash) => {
            let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("blake3 {}\u{2026}{}", &hex[..4], &hex[hex.len() - 4..])
        }
        QuarantineKey::Stamp { .. } => "size and date".to_owned(),
    }
}
