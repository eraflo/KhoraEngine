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

use crate::commands::assets_config::AssetManifest;
use crate::helpers::*;
use anyhow::{Context, Result};
use bincode;
use khora_sdk::prelude::{AssetMetadata, AssetSource, AssetUUID};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn pack() -> Result<()> {
    print_task_start("Packing Assets", ROCKET, MAGENTA);

    let manifest = load_manifest()?;
    let dest_dir = PathBuf::from(".dist/assets");
    fs::create_dir_all(&dest_dir)?;

    let valid_source_dirs: Vec<PathBuf> = manifest
        .source_directories
        .into_iter()
        .filter(|dir| dir.exists())
        .collect();

    if valid_source_dirs.is_empty() {
        print_error("No valid source directories found. Nothing to pack.");
        return Ok(());
    }

    let asset_files = find_asset_files(&valid_source_dirs)?;
    if asset_files.is_empty() {
        print_success("No asset files found to pack.");
        return Ok(());
    }

    println!(
        "{}🔎 Found:{} {} potential asset files to process.",
        BOLD,
        RESET,
        asset_files.len()
    );

    // This single function now handles the core logic.
    build_packfiles(&asset_files, &dest_dir)?;

    print_success("Asset pipeline finished successfully.");
    Ok(())
}

/// Builds the `data.pack` and `index.bin` files from the list of source assets.
fn build_packfiles(asset_files: &[PathBuf], dest_dir: &Path) -> Result<()> {
    let index_path = dest_dir.join("index.bin");
    let data_path = dest_dir.join("data.pack");

    let mut data_file = File::create(&data_path)
        .with_context(|| format!("Failed to create data pack at '{}'", data_path.display()))?;

    let mut final_metadata = Vec::new();
    let mut current_offset = 0;

    println!("{}📦 Packing asset data...", BOLD);

    for asset_path in asset_files {
        let asset_bytes = fs::read(asset_path)
            .with_context(|| format!("Failed to read asset file '{}'", asset_path.display()))?;
        let size = asset_bytes.len() as u64;

        // Write data to the packfile
        data_file.write_all(&asset_bytes)?;

        // --- Generate Metadata ---
        let path_str = asset_path.to_str().context("Invalid path encoding")?;
        let uuid = AssetUUID::new_v5(path_str);
        let asset_type_name = asset_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let mut variants = HashMap::new();
        variants.insert(
            "default".to_string(),
            AssetSource::Packed {
                offset: current_offset,
                size,
            },
        );

        final_metadata.push(AssetMetadata {
            uuid,
            source_path: asset_path.clone(),
            asset_type_name,
            dependencies: Vec::new(),
            variants,
            tags: Vec::new(),
        });

        current_offset += size;
    }

    println!("{}💾 Writing index file...", BOLD);
    let config = bincode::config::standard();
    let encoded_index = bincode::serde::encode_to_vec(&final_metadata, config)
        .context("Failed to serialize final metadata")?;

    fs::write(&index_path, &encoded_index)
        .with_context(|| format!("Failed to write index file to '{}'", index_path.display()))?;

    println!(
        "{}{} {} Wrote {} metadata entries to '{}' ({:.2} KB)",
        BOLD,
        GREEN,
        CHECK,
        final_metadata.len(),
        index_path.display(),
        encoded_index.len() as f64 / 1024.0
    );
    println!(
        "{}{} {} Wrote asset data to '{}' ({:.2} MB)",
        BOLD,
        GREEN,
        CHECK,
        data_path.display(),
        current_offset as f64 / (1024.0 * 1024.0)
    );

    Ok(())
}

/// Loads the `Assets.toml` manifest from the workspace root.
/// If the file does not exist, it returns the default configuration.
fn load_manifest() -> Result<AssetManifest> {
    let manifest_path = Path::new("Assets.toml");
    let manifest: AssetManifest = if manifest_path.exists() {
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
            .with_context(|| format!("Failed to parse TOML from '{}'", manifest_path.display()))?
    } else {
        println!(
            "{}💡 Info:{} No '{}' found. Using default configuration.",
            BOLD,
            RESET,
            manifest_path.display()
        );
        AssetManifest::default()
    };

    Ok(manifest)
}

/// Recursively finds all files in the given source directories.
fn find_asset_files(source_dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for dir in source_dirs {
        for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok())
        // Ignore errors for now
        {
            if entry.file_type().is_file() {
                files.push(entry.into_path());
            }
        }
    }
    Ok(files)
}
