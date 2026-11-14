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
use khora_sdk::khora_core::asset::CompressionKind;
use khora_sdk::khora_core::physics::PhysicsSettings;
use khora_sdk::{
    PackBuilder, PackSigner, RuntimeConfig, DEFAULT_SCENE_REL_PATH, RUNTIME_CONFIG_FILE,
};
use std::path::{Path, PathBuf};

mod cargo_build;
use cargo_build::stage_with_cargo_build;

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

    /// Compression scheme applied at pack time.
    pub fn compression(self) -> CompressionKind {
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

    /// Sub-directory name for the staged build (e.g. `windows` →
    /// `<project>/dist/windows/`).
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }

    /// Filename suffix appended to the runtime binary on this target.
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
}

/// Stages a build for the host OS with the **Release** preset, signed with
/// the user's key.
pub fn build_for_host(pvfs: &ProjectVfs, project_name: &str) -> Result<BuildOutcome> {
    let target = BuildTarget::host();
    let signer = user_signer()?;
    build_for_target(pvfs, project_name, target, BuildPreset::Release, &signer)
}

/// The key the user's builds are signed with, kept in
/// `user_dirs::signing_dir` (made on the first build, never in a project).
fn user_signer() -> Result<PackSigner> {
    let dir = khora_sdk::user_dirs::signing_dir(&|key| std::env::var_os(key))
        .ok_or_else(|| anyhow!("Build Game: no home folder to keep the signing key in"))?;
    PackSigner::load_or_create(&dir).context("Build Game: the signing key")
}

/// Stages a build for `target` with an explicit preset.
///
/// A non-host target fails at the runtime-binary lookup, or is refused by
/// [`stage_with_cargo_build`]: there is no cross-compilation.
pub fn build_for_target(
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
    std::fs::create_dir_all(&output_dir).with_context(|| {
        format!(
            "Failed to create build output directory {}",
            output_dir.display()
        )
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

    let outcome = match strategy {
        BuildStrategy::RuntimeStamp => {
            let runtime_src = locate_runtime_binary(target)
                .with_context(|| format!("Could not locate khora-runtime for {:?}", target))?;
            build_with_runtime(
                &pvfs.assets_root,
                &output_dir,
                project_name,
                target,
                preset,
                signer,
                &runtime_src,
            )?
        }
        BuildStrategy::CargoBuild => {
            let pack_out = pack_and_sign(&pvfs.assets_root, &output_dir, preset, signer)?;
            let bin_dst = stage_with_cargo_build(&pvfs.root, &output_dir, project_name, target)?;
            stamp_binary(&bin_dst, signer)?;
            BuildOutcome {
                strategy,
                output_dir: output_dir.clone(),
                binary_path: bin_dst,
                asset_count: pack_out.asset_count,
                pack_bytes: pack_out.pack_bytes(),
            }
        }
    };

    // The built game is the project's, at the version its `project.json`
    // gives.
    let game = khora_sdk::GameIdentity {
        name: project_name.to_owned(),
        version: crate::project_identity::project_version(&pvfs.root),
    };
    write_runtime_config_of(&output_dir, &game, preset, pvfs.physics_settings())?;

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
fn pack_and_sign(
    assets_root: &Path,
    output_dir: &Path,
    preset: BuildPreset,
    signer: &PackSigner,
) -> Result<khora_sdk::PackOutput> {
    PackBuilder::new(assets_root, output_dir)
        .with_compression(preset.compression())
        .with_signer(signer)
        .build()
        .context("PackBuilder failed")
}

/// Stamps `signer`'s public key into the staged binary at `bin`: the key
/// its runtime verifies the pack with.
fn stamp_binary(bin: &Path, signer: &PackSigner) -> Result<()> {
    khora_sdk::stamp_pack_key(bin, &signer.public_key())
        .with_context(|| format!("Build Game: cannot stamp {}", bin.display()))
}

/// Builds by runtime stamp: packs `assets_root` into `output_dir` and stages `runtime_src` there.
///
/// The pack has `preset`'s compression and is signed by `signer`; the binary
/// is named after the project and stamped with `signer`'s public key.
fn build_with_runtime(
    assets_root: &Path,
    output_dir: &Path,
    project_name: &str,
    target: BuildTarget,
    preset: BuildPreset,
    signer: &PackSigner,
    runtime_src: &Path,
) -> Result<BuildOutcome> {
    let pack_out = pack_and_sign(assets_root, output_dir, preset, signer)?;
    let bin_dst = stage_runtime(runtime_src, output_dir, project_name, target)?;
    stamp_binary(&bin_dst, signer)?;
    Ok(BuildOutcome {
        strategy: BuildStrategy::RuntimeStamp,
        output_dir: output_dir.to_path_buf(),
        binary_path: bin_dst,
        asset_count: pack_out.asset_count,
        pack_bytes: pack_out.pack_bytes(),
    })
}

/// Copies the pre-built runtime at `runtime_src` into `output_dir`, renamed
/// to the project name. Returns the absolute path of the staged binary.
fn stage_runtime(
    runtime_src: &Path,
    output_dir: &Path,
    project_name: &str,
    target: BuildTarget,
) -> Result<PathBuf> {
    let safe_name = sanitize_binary_name(project_name);
    let bin_filename = format!("{}{}", safe_name, target.exe_suffix());
    let bin_dst = output_dir.join(&bin_filename);
    std::fs::copy(runtime_src, &bin_dst).with_context(|| {
        format!(
            "Failed to copy runtime {} → {}",
            runtime_src.display(),
            bin_dst.display()
        )
    })?;
    set_executable_bit(&bin_dst);
    Ok(bin_dst)
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
/// underscores, for a binary file name.
fn sanitize_binary_name(name: &str) -> String {
    let cleaned: String = name
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
        "game".to_owned()
    } else {
        cleaned
    }
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
    let p = khora_sdk::user_dirs::khora_dir(&|key| std::env::var_os(key))?.join("engines");
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// Recursive file search. Returns `Some(path)` for the first hit.
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
