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

//! The cargo strategy of "Build Game": a project with its own `Cargo.toml`
//! is built by cargo, and its binary staged beside the pack.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use super::{sanitize_binary_name, set_executable_bit, BuildTarget};

/// Builds the project with `cargo build --release` and stages its binary in `output_dir`.
///
/// Cargo's output goes to the editor log; the binary is renamed to the
/// project name. Refuses a non-host target: there is no cross-compilation.
pub(super) fn stage_with_cargo_build(
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
