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

//! `cargo xtask assets pack`: every directory `Assets.toml` names, packed by
//! `khora-pack` (a binary of `khora-io`) into `.dist/assets/<name>`.
//!
//! The packing itself is the engine's, run as its own program: `xtask`
//! depends on no engine crate, so building it — the first thing `cargo gate`
//! does — never compiles the engine.

use crate::commands::assets_config::AssetManifest;
use crate::exec::execute_command;
use crate::term::*;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn pack() -> Result<()> {
    print_task_start("Packing Assets", ROCKET, MAGENTA);

    let manifest = load_manifest()?;
    let roots: Vec<PathBuf> = manifest
        .source_directories
        .into_iter()
        .filter(|dir| dir.exists())
        .collect();
    if roots.is_empty() {
        print_error("No valid source directories found. Nothing to pack.");
        return Ok(());
    }

    for root in &roots {
        let out = PathBuf::from(".dist/assets").join(pack_name(root));
        let (root_arg, out_arg) = (root.to_string_lossy(), out.to_string_lossy());
        execute_command(
            "cargo",
            &[
                "run",
                "--quiet",
                "--package",
                "khora-io",
                "--bin",
                "khora-pack",
                "--",
                &root_arg,
                &out_arg,
            ],
            "Pack",
        )?;
    }

    print_success("Asset pipeline finished successfully.");
    Ok(())
}

/// What a pack of `root` is called: the project it belongs to — the
/// directory holding `assets/` — else the directory itself.
fn pack_name(root: &Path) -> String {
    let named = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    let own = named(root).unwrap_or_else(|| "assets".to_owned());
    if own == "assets" {
        root.parent().and_then(named).unwrap_or(own)
    } else {
        own
    }
}

/// Loads the `Assets.toml` manifest from the workspace root.
/// If the file does not exist, it returns the default configuration.
fn load_manifest() -> Result<AssetManifest> {
    let manifest_path = Path::new("Assets.toml");
    if !manifest_path.exists() {
        println!(
            "{}💡 Info:{} No '{}' found. Using default configuration.",
            BOLD,
            RESET,
            manifest_path.display()
        );
        return Ok(AssetManifest::default());
    }
    println!(
        "{}💡 Info:{} Found '{}'. Loading configuration.",
        BOLD,
        RESET,
        manifest_path.display()
    );
    let manifest_str = fs::read_to_string(manifest_path).with_context(|| {
        format!(
            "Failed to read manifest file at '{}'",
            manifest_path.display()
        )
    })?;
    toml::from_str(&manifest_str)
        .with_context(|| format!("Failed to parse TOML from '{}'", manifest_path.display()))
}

#[cfg(test)]
mod tests {
    use super::pack_name;
    use std::path::Path;

    #[test]
    fn a_pack_is_named_after_the_project_holding_its_assets() {
        assert_eq!(pack_name(Path::new("examples/sandbox/assets")), "sandbox");
        assert_eq!(pack_name(Path::new("game/content")), "content");
        assert_eq!(pack_name(Path::new("assets")), "assets");
    }
}
