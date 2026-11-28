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

//! "Build Game": packs the project's assets and stages a runnable binary.
//!
//! A project without a `Cargo.toml` stamps the pre-built runtime; one with it
//! is built by cargo, host only. See the book: reference/editor.md, "09 — Build Game".

use crate::project_vfs::ProjectVfs;
use anyhow::{anyhow, Context, Result};
use khora_sdk::fs::Reclaimed;
use khora_sdk::khora_core::asset::CompressionKind;
use khora_sdk::khora_core::physics::PhysicsSettings;
use khora_sdk::trust::Refusals;
use khora_sdk::{
    LeftOutFile, PackBuilder, PackSigner, RuntimeConfig, DEFAULT_SCENE_REL_PATH,
    RUNTIME_CONFIG_FILE,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod cargo_build;
mod runtime_stamp;
use cargo_build::stage_with_cargo_build;
use runtime_stamp::{build_with_runtime, RuntimeStampBuild};

/// Build profile selecting the pack's compression; every preset signs its pack.
///
/// `Debug` / `Shipping` are reserved for a "Build…" dialog; `Release` is the
/// default until it exists.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPreset {
    /// Fast iteration: no compression.
    Debug,
    /// Production-bound: LZ4 per-entry compression.
    Release,
    /// Shipping: the same pack as Release.
    Shipping,
}

impl BuildPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
            Self::Shipping => "shipping",
        }
    }

    /// Returns the compression scheme applied at pack time.
    pub(crate) fn compression(self) -> CompressionKind {
        match self {
            Self::Debug => CompressionKind::None,
            Self::Release | Self::Shipping => CompressionKind::Lz4,
        }
    }
}

/// One of the platforms the editor knows how to stage a build for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTarget {
    Windows,
    Linux,
    Macos,
}

impl BuildTarget {
    /// Returns the build target matching the editor's host OS.
    pub fn host() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            // Treat every other unix-ish target as Linux for staging
            // purposes — the runtime binary suffix is the same.
            Self::Linux
        }
    }

    /// Returns the sub-directory name for the staged build (e.g. `windows` →
    /// `<project>/dist/windows/`).
    pub(crate) fn dir_name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }

    /// Returns the filename suffix appended to the runtime binary on this
    /// target.
    pub fn exe_suffix(self) -> &'static str {
        match self {
            Self::Windows => ".exe",
            Self::Linux | Self::Macos => "",
        }
    }
}

/// Which build strategy was used to produce a [`BuildOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStrategy {
    /// The pre-built `khora-runtime` binary, stamped next to the project's pack.
    RuntimeStamp,
    /// A binary built by `cargo build --release` from the project's
    /// `Cargo.toml`, linking its native Rust; host only.
    CargoBuild,
}

impl BuildStrategy {
    pub fn label(self) -> &'static str {
        match self {
            Self::RuntimeStamp => "runtime stamp",
            Self::CargoBuild => "cargo build",
        }
    }
}

/// Result of a successful build, which the menu dispatcher reports.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub strategy: BuildStrategy,
    pub output_dir: PathBuf,
    /// Absolute path of the staged executable.
    pub binary_path: PathBuf,
    pub asset_count: usize,
    pub pack_bytes: u64,
    /// The external files the pack left out, sorted by path: refused, over
    /// their container's cap, or missing.
    pub left_out: Vec<LeftOutFile>,
    /// What an interrupted build left beside the build's folder, and what was
    /// done with it; the previous build when it could not be removed.
    pub reclaimed: Vec<Reclaimed>,
}

impl BuildOutcome {
    /// Returns the outcome of the same build once its folder is moved to `output_dir`.
    fn moved_to(self, output_dir: &Path) -> Self {
        let binary_path = match self.binary_path.file_name() {
            Some(name) => output_dir.join(name),
            None => self.binary_path,
        };
        Self {
            output_dir: output_dir.to_path_buf(),
            binary_path,
            ..self
        }
    }
}

/// Returns the line a build's result is reported by: built, and what it left out, or why it failed.
///
/// A build that left files out says how many and names each, with why; one
/// that reclaimed what an interrupted build left says what it did with it.
pub(crate) fn result_line(result: &Result<BuildOutcome>) -> String {
    let out = match result {
        Ok(out) => out,
        Err(error) => return format!("Build Game: failed — {error:#}"),
    };
    let built = format!(
        "via {} — {} ({} assets, {} bytes packed)",
        out.strategy.label(),
        out.output_dir.display(),
        out.asset_count,
        out.pack_bytes
    );
    let line = match out.left_out.len() {
        0 => format!("Build Game: built {built}"),
        count => {
            let files = if count == 1 { "file" } else { "files" };
            let listed: Vec<String> = out.left_out.iter().map(ToString::to_string).collect();
            format!(
                "Build Game: built, {count} {files} left out, {built}: {}",
                listed.join("; ")
            )
        }
    };
    format!("{line}{}", reclaimed_note(&out.reclaimed))
}

/// Stages a build for the host OS with the **Release** preset, signed with
/// the user's key.
pub(crate) fn build_for_host(pvfs: &ProjectVfs, project_name: &str) -> Result<BuildOutcome> {
    let target = BuildTarget::host();
    let signer = user_signer()?;
    build_for_target(pvfs, project_name, target, BuildPreset::Release, &signer)
}

/// Loads the key the user's builds are signed with, kept in
/// `user_dirs::signing_dir` (made on the first build, never in a project).
fn user_signer() -> Result<PackSigner> {
    let dir = khora_sdk::user_dirs::signing_dir(&khora_sdk::user_dirs::process_env)
        .ok_or_else(|| anyhow!("Build Game: no home folder to keep the signing key in"))?;
    PackSigner::load_or_create(&dir).context("Build Game: the signing key")
}

/// Stages a build for `target` with an explicit preset under `<project>/dist/<target>/`.
///
/// The build is made in a temporary folder beside it, and swapped in whole
/// once it succeeded: a failed build leaves the previous one as it was, and
/// its temporary folder removed. What an interrupted build left beside the
/// folder is reclaimed first, the previous build it moved aside put back; the
/// outcome says what was, and so does the error of a build that then fails.
///
/// One build of a target at a time, across processes: the build holds a
/// [`PathLock`](khora_sdk::fs::PathLock) on the folder from start to end
/// (`dist/<target>.lock`, left in place). A second build of the target, in
/// this editor or another, is refused at once rather than waiting: a build
/// runs on the frame, and waiting would freeze the editor behind another's.
///
/// # Errors
///
/// When the project's name is blank; when another build of `target` is
/// running (in this process or another); when the runtime binary is not found or
/// cargo fails; when the pack fails (an asset refused, its message carried);
/// when a file cannot be written or the folder swapped in. A non-host target
/// fails at the runtime-binary lookup, or is refused by
/// [`stage_with_cargo_build`]: there is no cross-compilation.
pub(crate) fn build_for_target(
    pvfs: &ProjectVfs,
    project_name: &str,
    target: BuildTarget,
    preset: BuildPreset,
    signer: &PackSigner,
) -> Result<BuildOutcome> {
    if project_name.trim().is_empty() {
        anyhow::bail!("Build Game: project_name is empty");
    }

    let output_dir = pvfs.root.join("dist").join(target.dir_name());
    // Held to the end of the build, the swap and the reclaim of what an
    // interrupted build left included: a live build's folder is never taken
    // for a leftover.
    let _held = khora_sdk::fs::PathLock::try_hold(&output_dir).map_err(|error| {
        let left = format!("Build Game: {} is left as it was", output_dir.display());
        match error.kind() {
            std::io::ErrorKind::ResourceBusy => anyhow::anyhow!(
                "{left}: another build of it is running, in this editor or another ({error})"
            ),
            _ => anyhow::Error::new(error).context(left),
        }
    })?;
    let has_cargo = pvfs.root.join("Cargo.toml").is_file();
    let strategy = if has_cargo {
        BuildStrategy::CargoBuild
    } else {
        BuildStrategy::RuntimeStamp
    };

    log::info!(
        "Build Game: target={:?}, preset={}, strategy={}, output={}",
        target,
        preset.label(),
        strategy.label(),
        output_dir.display()
    );

    // The built game is the project's, at the version its `project.json`
    // gives.
    let game = khora_sdk::GameIdentity {
        name: project_name.to_owned(),
        version: crate::project_identity::project_version(&pvfs.reads().guard(&pvfs.root)),
    };
    let physics = pvfs.physics_settings();
    // Built beside the previous build and swapped in whole: a failed build
    // leaves the previous one as it was.
    let (outcome, reclaimed) =
        khora_sdk::fs::replace_dir_with_reporting(&output_dir, |staged| -> Result<BuildOutcome> {
            let outcome = match strategy {
                BuildStrategy::RuntimeStamp => {
                    let runtime_src = locate_runtime_binary(target).with_context(|| {
                        format!("Could not locate khora-runtime for {:?}", target)
                    })?;
                    build_with_runtime(RuntimeStampBuild {
                        assets_root: &pvfs.assets_root,
                        refusals: pvfs.reads().refusals(),
                        output_dir: staged,
                        project_name,
                        target,
                        preset,
                        signer,
                        runtime_src: &runtime_src,
                    })?
                }
                BuildStrategy::CargoBuild => {
                    let pack_out = pack_and_sign(
                        &pvfs.assets_root,
                        pvfs.reads().refusals(),
                        staged,
                        preset,
                        signer,
                    )?;
                    let bin_dst = stage_with_cargo_build(&pvfs.root, staged, project_name, target)?;
                    stamp_binary(&bin_dst, signer)?;
                    BuildOutcome {
                        strategy,
                        output_dir: staged.to_path_buf(),
                        binary_path: bin_dst,
                        asset_count: pack_out.asset_count,
                        pack_bytes: pack_out.pack_bytes(),
                        left_out: pack_out.left_out,
                        reclaimed: Vec::new(),
                    }
                }
            };
            write_runtime_config_of(staged, &game, preset, physics)?;
            Ok(outcome)
        });
    let built = outcome.with_context(|| {
        format!(
            "Build Game: {} is left as it was{}",
            output_dir.display(),
            reclaimed_note(&reclaimed)
        )
    })?;
    let outcome = BuildOutcome {
        reclaimed,
        ..built.moved_to(&output_dir)
    };

    log::info!(
        "Build Game: staged {} ({} assets, {} bytes packed, signed) via {}",
        outcome.binary_path.display(),
        outcome.asset_count,
        outcome.pack_bytes,
        strategy.label()
    );
    Ok(outcome)
}

/// Packs `assets_root` into `output_dir` — `index.bin`, `data.pack` and the
/// `manifest.bin` `signer` signs over them — with `preset`'s compression.
///
/// What the build refuses is reported to `refusals`, the run's.
fn pack_and_sign(
    assets_root: &Path,
    refusals: Arc<Refusals>,
    output_dir: &Path,
    preset: BuildPreset,
    signer: &PackSigner,
) -> Result<khora_sdk::PackOutput> {
    PackBuilder::new(assets_root, output_dir)
        .with_compression(preset.compression())
        .with_signer(signer)
        .with_refusals(refusals)
        .build()
        .context("PackBuilder failed")
}

/// Stamps `signer`'s public key into the staged binary at `bin`: the key
/// its runtime verifies the pack with.
fn stamp_binary(bin: &Path, signer: &PackSigner) -> Result<()> {
    khora_sdk::stamp_pack_key(bin, &signer.public_key())
        .with_context(|| format!("Build Game: cannot stamp {}", bin.display()))
}

/// Returns what a build did with an interrupted build's leftovers, as the end of a sentence; empty for nothing.
fn reclaimed_note(reclaimed: &[Reclaimed]) -> String {
    if reclaimed.is_empty() {
        return String::new();
    }
    let listed: Vec<String> = reclaimed.iter().map(ToString::to_string).collect();
    format!(" ({})", listed.join("; "))
}

/// `runtime.json` for a project with the default physics settings.
#[cfg(test)]
fn write_runtime_config(output_dir: &Path, project_name: &str, preset: BuildPreset) -> Result<()> {
    write_runtime_config_with(output_dir, project_name, preset, PhysicsSettings::default())
}

/// Returns the `runtime.json` "Build Game" writes for `game` built with `preset`.
///
/// Named and titled by the game's name, at its version, with the project's
/// physics settings.
fn runtime_config(
    game: &khora_sdk::GameIdentity,
    preset: BuildPreset,
    physics: PhysicsSettings,
) -> RuntimeConfig {
    RuntimeConfig {
        project_name: game.name.clone(),
        default_scene: DEFAULT_SCENE_REL_PATH.to_owned(),
        window_title: Some(game.name.clone()),
        preset: Some(preset.label().to_owned()),
        physics,
        game_version: game.version.clone(),
    }
}

/// `runtime.json` for a project named `project_name` that gives no version
/// of its own, the project's physics settings included.
#[cfg(test)]
fn write_runtime_config_with(
    output_dir: &Path,
    project_name: &str,
    preset: BuildPreset,
    physics: PhysicsSettings,
) -> Result<()> {
    let game = khora_sdk::GameIdentity {
        name: project_name.to_owned(),
        version: khora_sdk::DEFAULT_GAME_VERSION.to_owned(),
    };
    write_runtime_config_of(output_dir, &game, preset, physics)
}

/// Writes the `runtime.json` the runtime reads at start-up: the game it runs,
/// at its version, the project's physics settings included.
fn write_runtime_config_of(
    output_dir: &Path,
    game: &khora_sdk::GameIdentity,
    preset: BuildPreset,
    physics: PhysicsSettings,
) -> Result<()> {
    let cfg = runtime_config(game, preset, physics);
    let cfg_text = serde_json::to_string_pretty(&cfg).context("serialize runtime.json")?;
    std::fs::write(output_dir.join(RUNTIME_CONFIG_FILE), cfg_text)
        .context("Failed to write runtime.json")
}

#[cfg_attr(not(unix), allow(unused_variables))]
fn set_executable_bit(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

/// Returns the project name with filesystem-unsafe characters made
/// underscores, for a binary file name every system makes.
///
/// A name Windows keeps for a device (`con`, `nul`, …) gets a `_` after it,
/// as [`crate::project_vfs::check_new_name`] refuses it.
fn sanitize_binary_name(name: &str) -> String {
    let mut cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        return "game".to_owned();
    }
    if crate::project_vfs::check_new_name(&cleaned).is_err() {
        cleaned.push('_');
    }
    cleaned
}

/// Returns the `khora-runtime` binary for `target`.
///
/// Looked for beside the editor binary (a release archive, or
/// `target/<profile>/`), then in the other cargo profile, then in the hub's
/// engine cache (`~/.khora/engines/<version>/runtime/`). Only the host's
/// runtime is found: the hub and dev builds fetch or build no other.
fn locate_runtime_binary(target: BuildTarget) -> Result<PathBuf> {
    let bin_name = format!("khora-runtime{}", target.exe_suffix());

    // First, beside the editor binary — release-archive layout, also what
    // `cargo build -p khora-runtime` produces in `target/<profile>/`.
    let sibling_dir = if target == BuildTarget::host() {
        let editor_exe =
            std::env::current_exe().context("locate_runtime_binary: current_exe failed")?;
        editor_exe.parent().map(|p| p.to_path_buf())
    } else {
        None
    };
    if let Some(dir) = sibling_dir.as_ref() {
        let candidate = dir.join(&bin_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    // Then the sibling profile fallback — for the common dev workflow where the
    // hub launches an editor in `target/debug/` while the contributor
    // ran the hub itself in `--release`. Same layout, just the other
    // profile.
    if let Some(dir) = sibling_dir.as_ref() {
        if let Some(workspace_root) = workspace_root_from_target_dir(dir) {
            for profile in ["release", "debug"] {
                let candidate = workspace_root.join("target").join(profile).join(&bin_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    // Then the hub's engine cache — matches the layout produced by
    // `hub::download::start_download`: <cache>/<version>/runtime/<bin>.
    if target == BuildTarget::host() {
        if let Some(cache) = engine_cache_dir() {
            if let Ok(entries) = std::fs::read_dir(&cache) {
                let mut versions: Vec<PathBuf> = entries
                    .flatten()
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .map(|e| e.path())
                    .collect();
                versions.sort();
                for ver in versions.iter().rev() {
                    let canonical = ver.join("runtime").join(&bin_name);
                    if canonical.is_file() {
                        return Ok(canonical);
                    }
                    if let Some(found) = find_file_recursive(ver, &bin_name) {
                        return Ok(found);
                    }
                }
            }
        }
    }

    // Pedagogical error: the contributor most likely just hasn't built
    // the runtime yet. Point them at the exact command (cargo hub-dev
    // does it transparently as part of the dev workflow).
    let workspace_hint = sibling_dir
        .as_ref()
        .and_then(|d| workspace_root_from_target_dir(d))
        .map(|w| {
            format!(
                " Run `cargo hub-dev` (or `cargo build -p khora-runtime`) from {}.",
                w.display()
            )
        })
        .unwrap_or_default();
    Err(anyhow!(
        "khora-runtime binary not found for {:?}. \
         Expected '{}' next to the editor binary or under \
         '~/.khora/engines/<version>/runtime/'.{}",
        target,
        bin_name,
        workspace_hint
    ))
}

/// Returns the workspace root if `target_dir` is a `target/<profile>/` of one.
///
/// Its parent must be named `target`, and its grandparent hold a `Cargo.toml`.
fn workspace_root_from_target_dir(target_dir: &Path) -> Option<PathBuf> {
    let target_dir_name = target_dir.parent()?.file_name()?.to_str()?;
    if target_dir_name != "target" {
        return None;
    }
    let candidate = target_dir.parent()?.parent()?;
    if candidate.join("Cargo.toml").is_file() {
        Some(candidate.to_path_buf())
    } else {
        None
    }
}

/// Returns `~/.khora/engines/` if accessible. The hub manages this
/// directory; the editor reads from it.
fn engine_cache_dir() -> Option<PathBuf> {
    let p = khora_sdk::user_dirs::khora_dir(&khora_sdk::user_dirs::process_env)?.join("engines");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Searches `dir` recursively and returns the first file named `filename`.
fn find_file_recursive(dir: &Path, filename: &str) -> Option<PathBuf> {
    let direct = dir.join(filename);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(found) = find_file_recursive(&p, filename) {
                return Some(found);
            }
        } else if p.file_name().and_then(|n| n.to_str()) == Some(filename) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests;
