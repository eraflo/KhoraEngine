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

//! "Build Game" — packs the project's assets and stages a runnable binary.
//!
//! Two strategies, picked deterministically from the project's contents:
//!
//! - **Runtime stamp** (default, project has no `Cargo.toml`):
//!   1. `khora_io::PackBuilder` produces `index.bin` + `data.pack` from
//!      `<project>/assets/`.
//!   2. The pre-built `khora-runtime` binary for the chosen target is
//!      copied into the output directory and renamed to the project name.
//!   3. A `runtime.json` companion file is written so the runtime knows
//!      which scene to auto-load.
//!
//!   This path is **cross-platform trivial** — the runtime binary already
//!   exists for every target (built by `release.yml`), so building Linux
//!   from a Windows host is just a file copy.
//!
//! - **Cargo build** (project has `Cargo.toml` — opted in via the hub's
//!   "Add Native Code" button):
//!   1. Same `PackBuilder` step.
//!   2. `cargo build --release --manifest-path <project>/Cargo.toml`
//!      compiles the user's binary (which depends on `khora-sdk` and
//!      registers their custom components / agents / lanes).
//!   3. The compiled binary is copied into the output directory.
//!   4. Same `runtime.json` companion is written.
//!
//!   This path is **host-only in v1** because Rust cross-compilation needs
//!   per-target toolchains; running the editor on each target is the
//!   simplest workaround. Future work can integrate `cross` for
//!   Docker-based cross-compilation.
//!
//! Output layout (identical between the two strategies):
//! ```text
//! <project>/dist/<target>/
//! ├── <project_name>{.exe}   # renamed runtime OR compiled user binary
//! ├── data.pack              # asset blobs
//! ├── index.bin              # asset metadata (UUIDs → packed offsets)
//! └── runtime.json           # project name + default scene rel path
//! ```
//!
//! The user can therefore start data-only, ship cross-platform via the
//! stamp strategy, and "graduate" to native Rust without changing how
//! Build Game is invoked — the editor switches strategies automatically
//! based on `Cargo.toml`'s presence.

use crate::project_vfs::ProjectVfs;
use anyhow::{anyhow, Context, Result};
use khora_sdk::khora_core::asset::CompressionKind;
use khora_sdk::PackBuilder;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Build profile selecting compression / manifest / runtime-validation
/// trade-offs in one place. Callers pick the preset; the build pipeline
/// reads the resolved settings off it.
///
/// `Debug` / `Shipping` are reserved for the upcoming "Build…" dialog
/// (Phase 6) — `Release` is the default until that lands.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildPreset {
    /// Fast iteration. No compression, no manifest, runtime validation
    /// stays on so corruption is loud.
    Debug,
    /// Production-bound. LZ4 per-entry compression, manifest emitted,
    /// runtime validation on (catches broken downloads before crashes).
    Release,
    /// Performance-critical shipping. Same on-disk format as Release but
    /// the runtime *skips* integrity verification at load time — used
    /// when QA has already validated the pack and you want maximum FPS.
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

    /// Whether to emit a `manifest.bin` BLAKE3 sidecar.
    pub fn emit_manifest(self) -> bool {
        match self {
            Self::Debug => false,
            Self::Release | Self::Shipping => true,
        }
    }

    /// Whether the staged runtime should hash assets on load and bail
    /// on mismatch.
    pub fn verify_integrity(self) -> bool {
        match self {
            Self::Debug | Self::Release => true,
            Self::Shipping => false,
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
    /// The build target matching the editor's host OS. Used by the
    /// "Build Game…" menu (v1) and as a sane default in any future UI.
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

/// What we write next to the staged runtime so it knows which scene to
/// auto-load. Mirrors the schema `khora_runtime::RuntimeConfig` reads.
#[derive(Debug, Serialize)]
struct RuntimeConfig<'a> {
    project_name: &'a str,
    default_scene: &'a str,
    /// Build preset label (debug/release/shipping). Runtime uses it to
    /// decide whether to verify pack integrity on load.
    preset: &'a str,
    /// Whether the runtime should re-hash assets against `manifest.bin`
    /// on load. Independent of `preset` so runtimes shipped before the
    /// preset concept existed can still toggle it.
    verify_integrity: bool,
}

/// Which build strategy was used to produce a [`BuildOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStrategy {
    /// The pre-built `khora-runtime` binary was stamped next to the
    /// project's pack. Cross-platform trivial.
    RuntimeStamp,
    /// `cargo build --release` was invoked on the project's `Cargo.toml`
    /// to produce a custom binary that links the user's native Rust.
    /// Host-only.
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

/// Result of a successful build, returned to the caller (the editor's
/// menu dispatcher logs + banners off these fields).
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub strategy: BuildStrategy,
    pub output_dir: PathBuf,
    /// Absolute path of the staged executable. Used by future UI affordances
    /// like a "Reveal in Explorer" button — currently the menu dispatcher
    /// only reads `output_dir` and the asset/byte counts.
    #[allow(dead_code)]
    pub binary_path: PathBuf,
    pub asset_count: usize,
    pub pack_bytes: u64,
}

/// Stages a build for the host OS using the **Release** preset by
/// default. Convenience wrapper for the menu's "Build Game…" entry.
pub fn build_for_host(pvfs: &ProjectVfs, project_name: &str) -> Result<BuildOutcome> {
    let target = BuildTarget::host();
    build_for_target(pvfs, project_name, target, BuildPreset::Release)
}

/// Stages a build for any target with an explicit preset. v1 only
/// invokes this with the host; non-host targets fail at the
/// runtime-binary lookup (or, for the cargo path, are explicitly
/// refused — see [`stage_with_cargo_build`]) until cross-compile lands.
pub fn build_for_target(
    pvfs: &ProjectVfs,
    project_name: &str,
    target: BuildTarget,
    preset: BuildPreset,
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

    // Pack assets directly into the output dir — index.bin + data.pack
    // (and optionally manifest.bin) sit alongside the staged binary
    // regardless of strategy. Compression / manifest are driven by the
    // build preset.
    let pack_out = PackBuilder::new(&pvfs.assets_root, &output_dir)
        .with_compression(preset.compression())
        .with_manifest(preset.emit_manifest())
        .build()
        .context("PackBuilder failed")?;

    let bin_dst = match strategy {
        BuildStrategy::RuntimeStamp => stage_with_runtime_stamp(&output_dir, project_name, target)?,
        BuildStrategy::CargoBuild => {
            stage_with_cargo_build(&pvfs.root, &output_dir, project_name, target)?
        }
    };

    write_runtime_config(&output_dir, project_name, preset)?;

    log::info!(
        "Build Game: staged {} ({} assets, {} bytes packed) via {}",
        bin_dst.display(),
        pack_out.asset_count,
        pack_out.pack_bytes(),
        strategy.label()
    );

    Ok(BuildOutcome {
        strategy,
        output_dir,
        binary_path: bin_dst,
        asset_count: pack_out.asset_count,
        pack_bytes: pack_out.pack_bytes(),
    })
}

/// Stamps the pre-built `khora-runtime` into `output_dir`, renamed to the
/// project name. Returns the absolute path of the staged binary.
fn stage_with_runtime_stamp(
    output_dir: &Path,
    project_name: &str,
    target: BuildTarget,
) -> Result<PathBuf> {
    let runtime_src = locate_runtime_binary(target)
        .with_context(|| format!("Could not locate khora-runtime for {:?}", target))?;
    let safe_name = sanitize_binary_name(project_name);
    let bin_filename = format!("{}{}", safe_name, target.exe_suffix());
    let bin_dst = output_dir.join(&bin_filename);
    std::fs::copy(&runtime_src, &bin_dst).with_context(|| {
        format!(
            "Failed to copy runtime {} → {}",
            runtime_src.display(),
            bin_dst.display()
        )
    })?;
    set_executable_bit(&bin_dst);
    Ok(bin_dst)
}

/// Invokes `cargo build --release` against `<project>/Cargo.toml`,
/// streams stdout+stderr into the editor log, and copies the resulting
/// binary into `output_dir` renamed to the project name.
///
/// Refuses non-host targets with a clear error — Rust cross-compilation
/// is host-only in v1.
fn stage_with_cargo_build(
    project_root: &Path,
    output_dir: &Path,
    project_name: &str,
    target: BuildTarget,
) -> Result<PathBuf> {
    if target != BuildTarget::host() {
        anyhow::bail!(
            "Build Game (cargo strategy): target {:?} is not the host \
             ({:?}). Native-Rust projects can only be built for the host \
             OS in v1 — run the editor on the desired target.",
            target,
            BuildTarget::host()
        );
    }

    let manifest = project_root.join("Cargo.toml");
    if !manifest.is_file() {
        anyhow::bail!(
            "Build Game (cargo strategy): no Cargo.toml at {}",
            manifest.display()
        );
    }

    log::info!(
        "Build Game: invoking `cargo build --release --manifest-path {}`",
        manifest.display()
    );

    let output = std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .with_context(|| {
            format!(
                "Failed to launch cargo (is it installed and on PATH?). \
                 Manifest: {}",
                manifest.display()
            )
        })?;

    // Stream cargo's stdout/stderr through the editor's logger so the
    // user sees compile errors in the console panel.
    if !output.stdout.is_empty() {
        log::info!("cargo stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        // Cargo writes "Compiling foo v0.1.0…" to stderr by design — log
        // at info, not warn.
        log::info!("cargo stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        anyhow::bail!(
            "cargo build failed with exit code {:?}",
            output.status.code()
        );
    }

    // Discover the produced binary in target/release/. The Cargo package
    // name is whatever the user set in their generated Cargo.toml — read
    // it back out of the manifest. We fall back to scanning the directory
    // when the [package].name TOML extraction stumbles on hand-edits.
    let target_release = project_root.join("target").join("release");
    let pkg_bin = find_compiled_binary(&target_release, target).with_context(|| {
        format!(
            "Could not find a compiled executable in {} (cargo build \
                 succeeded but the binary is missing — is `[[bin]]` set?)",
            target_release.display()
        )
    })?;

    let safe_name = sanitize_binary_name(project_name);
    let bin_filename = format!("{}{}", safe_name, target.exe_suffix());
    let bin_dst = output_dir.join(&bin_filename);
    std::fs::copy(&pkg_bin, &bin_dst).with_context(|| {
        format!(
            "Failed to copy compiled binary {} → {}",
            pkg_bin.display(),
            bin_dst.display()
        )
    })?;
    set_executable_bit(&bin_dst);
    Ok(bin_dst)
}

/// Walks `target/release/` for the first regular file with the host's
/// executable suffix, ignoring cargo metadata files (`.d`, `.rlib`, etc).
fn find_compiled_binary(target_release: &Path, target: BuildTarget) -> Result<PathBuf> {
    let suffix = target.exe_suffix();
    let entries = std::fs::read_dir(target_release).with_context(|| {
        format!(
            "Failed to read target/release directory {}",
            target_release.display()
        )
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        // Skip cargo metadata files.
        if name.ends_with(".d") || name.ends_with(".rlib") || name.ends_with(".pdb") {
            continue;
        }
        // On Windows, only files ending in .exe are the binary. On Unix,
        // the binary has no extension.
        if !suffix.is_empty() {
            if path.extension().and_then(|s| s.to_str()) == Some(suffix.trim_start_matches('.')) {
                return Ok(path);
            }
        } else if path.extension().is_none() {
            return Ok(path);
        }
    }
    Err(anyhow!(
        "no compiled executable found in {}",
        target_release.display()
    ))
}

fn write_runtime_config(output_dir: &Path, project_name: &str, preset: BuildPreset) -> Result<()> {
    let cfg = RuntimeConfig {
        project_name,
        default_scene: crate::scene_io::DEFAULT_SCENE_REL,
        preset: preset.label(),
        verify_integrity: preset.verify_integrity(),
    };
    let cfg_text = serde_json::to_string_pretty(&cfg).context("serialize runtime.json")?;
    std::fs::write(output_dir.join("runtime.json"), cfg_text)
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

/// Replaces filesystem-unsafe characters in the project name with
/// underscores so we can use it as a binary filename.
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

/// Looks for the `khora-runtime` binary for `target` in:
///
/// 1. **Sibling of the editor binary** — the canonical layout once the
///    engine release archive is unpacked, and what `cargo build` produces
///    in `target/<profile>/`.
/// 2. **Sibling profile fallback** — when the editor runs from
///    `target/release/`, also check `target/debug/` (and vice-versa).
///    Lets a contributor running `cargo run -p khora-hub --release` find
///    a runtime that was built in debug by `cargo build` or `cargo
///    hub-dev`.
/// 3. **Hub engine cache** — `~/.khora/engines/<version>/runtime/` is the
///    layout produced by `hub::download::start_download` when the matching
///    `khora-runtime-<host>` artifact is present in the GitHub release.
///
/// Non-host targets always fall through today: the hub only fetches its
/// own host architecture from a release, and dev builds only produce the
/// host runtime. Cross-target build-template caching is a future expansion
/// that re-uses this same lookup once the hub fetches all three runtime
/// archives.
fn locate_runtime_binary(target: BuildTarget) -> Result<PathBuf> {
    let bin_name = format!("khora-runtime{}", target.exe_suffix());

    // (1) Sibling of the editor binary — release-archive layout, also what
    //     `cargo build -p khora-runtime` produces in `target/<profile>/`.
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

    // (2) Sibling profile fallback — for the common dev workflow where the
    //     hub launches an editor in `target/debug/` while the contributor
    //     ran the hub itself in `--release`. Same layout, just the other
    //     profile.
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

    // (3) Hub engine cache — matches the layout produced by
    //     `hub::download::start_download`: <cache>/<version>/runtime/<bin>.
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

/// Returns the workspace root if `target_dir` is a `target/<profile>/`
/// subdirectory of one. Detection: parent must be named `target`, and
/// the grandparent must contain a `Cargo.toml`.
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
    let home = dirs_home()?;
    let p = home.join(".khora").join("engines");
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

fn dirs_home() -> Option<PathBuf> {
    // Avoid a hard dep on `dirs`; fall back to OS-standard env vars.
    if let Some(h) = std::env::var_os("HOME") {
        return Some(PathBuf::from(h));
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        return Some(PathBuf::from(p));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_matches_cfg() {
        let host = BuildTarget::host();
        if cfg!(target_os = "windows") {
            assert_eq!(host, BuildTarget::Windows);
        } else if cfg!(target_os = "macos") {
            assert_eq!(host, BuildTarget::Macos);
        } else {
            assert_eq!(host, BuildTarget::Linux);
        }
    }

    #[test]
    fn target_suffixes_are_correct() {
        assert_eq!(BuildTarget::Windows.exe_suffix(), ".exe");
        assert_eq!(BuildTarget::Linux.exe_suffix(), "");
        assert_eq!(BuildTarget::Macos.exe_suffix(), "");
    }

    #[test]
    fn sanitize_binary_name_strips_unsafe_chars() {
        assert_eq!(sanitize_binary_name("My Game!"), "My_Game_");
        assert_eq!(sanitize_binary_name("ok-name_1"), "ok-name_1");
        assert_eq!(sanitize_binary_name(""), "game");
        assert_eq!(sanitize_binary_name("/etc/passwd"), "_etc_passwd");
    }
}
