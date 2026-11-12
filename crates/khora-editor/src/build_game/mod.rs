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
use khora_sdk::khora_core::physics::PhysicsSettings;
use khora_sdk::{
    PackBuilder, PackSigner, RuntimeConfig, DEFAULT_SCENE_REL_PATH, RUNTIME_CONFIG_FILE,
};
use std::path::{Path, PathBuf};

mod cargo_build;
use cargo_build::stage_with_cargo_build;

/// Build profile selecting the pack's compression. Every preset signs its
/// pack and every runtime verifies it: a pack is sealed whatever the
/// preset.
///
/// `Debug` / `Shipping` are reserved for the upcoming "Build…" dialog
/// (Phase 6) — `Release` is the default until that lands.
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
    pub binary_path: PathBuf,
    pub asset_count: usize,
    pub pack_bytes: u64,
}

/// Stages a build for the host OS using the **Release** preset by
/// default, signed with the user's key. Convenience wrapper for the menu's
/// "Build Game…" entry.
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

/// Stages a build for any target with an explicit preset. v1 only
/// invokes this with the host; non-host targets fail at the
/// runtime-binary lookup (or, for the cargo path, are explicitly
/// refused — see [`stage_with_cargo_build`]) until cross-compile lands.
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

/// The runtime-stamp build over the runtime binary at `runtime_src`: the
/// project's `assets_root` packed into `output_dir` with `preset`'s
/// compression and signed by `signer`, and `runtime_src` staged there,
/// named after the project and stamped with `signer`'s public key.
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

/// The `runtime.json` "Build Game" writes for `game` built with `preset`:
/// named and titled by the game's name, at the game's version, with the
/// project's physics settings.
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

/// `runtime.json`: what the runtime reads at start-up — the game it runs,
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

    /// The `runtime.json` the runtime reads, for a project named "Khora Demo"
    /// built with the Release preset. The SDK's reader test parses the same
    /// file, so what is written here is what the runtime gets back.
    const READ_BY_THE_RUNTIME: &str =
        include_str!("../../../khora-sdk/tests/fixtures/runtime.json");

    /// Every field the runtime reads is written, with the value the runtime
    /// expects — the window title included.
    #[test]
    fn the_runtime_json_the_editor_writes_is_what_the_runtime_reads() -> Result<()> {
        let dir =
            std::env::temp_dir().join(format!("khora-editor-runtime-json-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let written = write_runtime_config(&dir, "Khora Demo", BuildPreset::Release)
            .and_then(|()| Ok(std::fs::read_to_string(dir.join("runtime.json"))?));
        let _ = std::fs::remove_dir_all(&dir);

        let written: serde_json::Value = serde_json::from_str(&written?)?;
        let expected: serde_json::Value = serde_json::from_str(READ_BY_THE_RUNTIME)?;
        assert_eq!(written, expected);
        Ok(())
    }

    /// **A project's physics step reaches the built game.** The editor writes
    /// the project's `physics` section into `runtime.json`, and the runtime
    /// reads that step back; a project at the default step writes none.
    #[test]
    fn the_runtime_json_carries_the_projects_physics_step_to_the_runtime() -> Result<()> {
        let a_128th = PhysicsSettings {
            step: std::time::Duration::from_nanos(7_812_500),
        };
        let dir = std::env::temp_dir().join(format!(
            "khora-editor-runtime-json-physics-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        let written = write_runtime_config_with(&dir, "Khora Demo", BuildPreset::Release, a_128th)
            .and_then(|()| Ok(std::fs::read_to_string(dir.join("runtime.json"))?));
        let _ = std::fs::remove_dir_all(&dir);

        let read: RuntimeConfig = serde_json::from_str(&written?)?;
        assert_eq!(
            read.physics, a_128th,
            "the runtime reads the project's step"
        );
        Ok(())
    }

    /// **Build Game writes the project's version.** The `runtime.json` a
    /// game is built with carries the project's version as its
    /// `game_version`, which the runtime reads back as the game's identity;
    /// the rest is what Build Game always wrote — the name as project name
    /// and window title, the preset, the physics step.
    #[test]
    fn build_game_writes_the_projects_version() -> Result<()> {
        let game = khora_sdk::GameIdentity {
            name: "Khora Demo".to_owned(),
            version: "2.3.4".to_owned(),
        };
        let a_128th = PhysicsSettings {
            step: std::time::Duration::from_nanos(7_812_500),
        };

        let cfg = runtime_config(&game, BuildPreset::Release, a_128th);

        assert_eq!(cfg.game_version, "2.3.4", "the project's version");
        assert_eq!(cfg.project_name, "Khora Demo");
        assert_eq!(cfg.window_title.as_deref(), Some("Khora Demo"));
        assert_eq!(cfg.default_scene, DEFAULT_SCENE_REL_PATH);
        assert_eq!(cfg.preset.as_deref(), Some(BuildPreset::Release.label()));
        assert_eq!(cfg.physics, a_128th);

        let read: RuntimeConfig = serde_json::from_str(&serde_json::to_string_pretty(&cfg)?)?;
        assert_eq!(
            read.game_identity(),
            game,
            "the runtime reads the game back from what Build Game writes"
        );
        Ok(())
    }

    /// The key in the one key slot `binary` holds, `None` when it holds no
    /// slot or several.
    fn key_in_slot(binary: &[u8]) -> Option<khora_sdk::PackKey> {
        let magic = khora_sdk::slot_magic();
        let mut slots = (0..binary.len().saturating_sub(64))
            .filter(|&at| binary[at] == magic[0] && binary[at..at + 32] == magic);
        let at = slots.next()?;
        if slots.next().is_some() {
            return None;
        }
        let key: [u8; 32] = binary[at + 32..at + 64].try_into().ok()?;
        Some(khora_sdk::PackKey(key))
    }

    /// **The build signs the pack and stamps the binary**, whatever the
    /// preset: over a fake runtime binary holding an empty key slot, a build
    /// signed by a key made in a temporary folder stages a binary whose slot
    /// holds that key, and the `manifest.bin` beside it verifies with the key
    /// read from the staged binary over the `index.bin` beside it, recording
    /// every packed file. The runtime binary built from is left unstamped.
    #[test]
    fn the_build_signs_the_pack_and_stamps_the_binary() -> Result<()> {
        use khora_sdk::khora_core::asset::AssetUUID;

        const FILES: &[(&str, &[u8])] = &[
            ("scenes/default.kscene", b"the default scene"),
            ("textures/a.png", b"a texture"),
        ];
        let root =
            std::env::temp_dir().join(format!("khora-editor-signed-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let assets = root.join("project").join("assets");
        for (rel, bytes) in FILES {
            let path = assets.join(rel);
            std::fs::create_dir_all(path.parent().expect("a folder"))?;
            std::fs::write(path, bytes)?;
        }
        let runtime_src = root.join(format!("khora-runtime{}", BuildTarget::host().exe_suffix()));
        let mut fake_runtime = b"a fake runtime's code, then its slot: ".to_vec();
        fake_runtime.extend_from_slice(&khora_sdk::slot_magic());
        fake_runtime.extend_from_slice(&[0; 32]);
        fake_runtime.extend_from_slice(b" and the rest of the fake runtime");
        std::fs::write(&runtime_src, &fake_runtime)?;

        let outcome = (|| -> Result<Vec<(BuildPreset, BuildOutcome, khora_sdk::PackKey)>> {
            let signer = PackSigner::load_or_create(&root.join("signing"))?;
            let mut built = Vec::new();
            for preset in [
                BuildPreset::Debug,
                BuildPreset::Release,
                BuildPreset::Shipping,
            ] {
                let output_dir = root.join("dist").join(preset.label());
                let outcome = build_with_runtime(
                    &assets,
                    &output_dir,
                    "Khora Demo",
                    BuildTarget::host(),
                    preset,
                    &signer,
                    &runtime_src,
                )?;
                built.push((preset, outcome, signer.public_key()));
            }
            Ok(built)
        })();
        let checked = outcome.and_then(|built| {
            for (preset, outcome, signer_key) in built {
                let staged = std::fs::read(&outcome.binary_path)?;
                let key = key_in_slot(&staged)
                    .ok_or_else(|| anyhow!("{preset:?}: the staged binary holds no one slot"))?;
                assert_eq!(
                    key, signer_key,
                    "{preset:?}: the slot holds the signer's key"
                );
                assert_eq!(
                    outcome.binary_path.file_name().and_then(|n| n.to_str()),
                    Some(format!("Khora_Demo{}", BuildTarget::host().exe_suffix()).as_str()),
                    "{preset:?}: the staged binary is named after the project"
                );

                let manifest_bin = std::fs::read(outcome.output_dir.join("manifest.bin"))?;
                let index_bin = std::fs::read(outcome.output_dir.join("index.bin"))?;
                let manifest = khora_sdk::SignedManifest::decode(&manifest_bin)
                    .map_err(|e| anyhow!("{preset:?}: manifest.bin is not signed: {e}"))?
                    .verify(&key, &index_bin)
                    .map_err(|e| anyhow!("{preset:?}: manifest.bin does not verify: {e}"))?;
                assert_eq!(manifest.len(), FILES.len(), "{preset:?}");
                for (rel, bytes) in FILES {
                    manifest
                        .verify(&AssetUUID::new_v5(rel), bytes)
                        .map_err(|e| anyhow!("{preset:?}: {rel} is not recorded: {e:#}"))?;
                }
            }
            Ok(std::fs::read(&runtime_src)?)
        });
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(
            checked?, fake_runtime,
            "the runtime binary built from was changed"
        );
        Ok(())
    }
}
