// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Project template creation for new Khora Engine projects.

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Minimal project descriptor persisted as `project.json` in the root of the project.
#[derive(Serialize)]
struct ProjectDescriptor<'a> {
    name: &'a str,
    engine_version: &'a str,
    created_at: u64,
}

/// Creates a new Khora Engine project on disk.
///
/// Directory layout:
/// ```text
/// <parent>/<name>/
///   project.json           ← project descriptor
///   assets/                ← asset root
///   assets/scenes/         ← default scene folder
///   assets/textures/
///   assets/meshes/
///   assets/audio/
///   assets/shaders/
/// ```
///
/// Returns the absolute path to the project root directory.
pub fn create_project(name: &str, parent: &Path, engine_version: &str) -> Result<PathBuf> {
    // Safety: strip any path-separator characters from the name.
    let safe_name = sanitize_name(name);
    if safe_name.is_empty() {
        anyhow::bail!("Project name is empty or contains only invalid characters");
    }

    let root = parent.join(&safe_name);

    if root.exists() {
        anyhow::bail!(
            "Directory '{}' already exists — choose a different name or location",
            root.display()
        );
    }

    // Create the directory tree.
    std::fs::create_dir_all(&root)
        .with_context(|| format!("Failed to create project directory '{}'", root.display()))?;

    for sub in &["scenes", "textures", "meshes", "audio", "shaders"] {
        std::fs::create_dir_all(root.join("assets").join(sub))
            .with_context(|| format!("Failed to create assets/{} directory", sub))?;
    }

    // Create src/ directory for user game code.
    std::fs::create_dir_all(root.join("src")).context("Failed to create src/ directory")?;

    // Write .gitignore.
    std::fs::write(root.join(".gitignore"), "target/\n*.lock\n")
        .context("Failed to write .gitignore")?;

    // Write project.json.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let descriptor = ProjectDescriptor {
        name,
        engine_version,
        created_at: now,
    };

    let json = serde_json::to_string_pretty(&descriptor)
        .context("Failed to serialize project descriptor")?;

    std::fs::write(root.join("project.json"), json).context("Failed to write project.json")?;

    // Write a default scene descriptor so the editor starts with Camera + Light.
    let default_scene = serde_json::json!({
        "name": "Default Scene",
        "entities": [
            {
                "name": "Main Camera",
                "type": "Camera",
                "position": [0.0, 5.0, 10.0]
            },
            {
                "name": "Directional Light",
                "type": "Light",
                "position": [0.0, 10.0, 0.0]
            }
        ]
    });
    let scene_json = serde_json::to_string_pretty(&default_scene)
        .context("Failed to serialize default scene")?;
    std::fs::write(
        root.join("assets")
            .join("scenes")
            .join("default.scene.json"),
        scene_json,
    )
    .context("Failed to write default scene")?;

    Ok(root)
}

/// Launches the Khora Editor with the given project path.
///
/// Spawns the editor process in the background and returns immediately.
pub fn launch_editor(editor_binary: &str, project_path: &Path) -> Result<()> {
    std::process::Command::new(editor_binary)
        .arg("--project")
        .arg(project_path)
        .spawn()
        .with_context(|| {
            format!(
                "Failed to launch editor at '{}' with project '{}'",
                editor_binary,
                project_path.display()
            )
        })?;
    Ok(())
}

/// Opens the project folder in the system file manager.
#[allow(dead_code)]
pub fn reveal_in_explorer(path: &Path) -> Result<()> {
    open::that(path).with_context(|| format!("Failed to open '{}'", path.display()))?;
    Ok(())
}

/// Strips characters that are unsafe in file/directory names.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ')
        .collect::<String>()
        .trim()
        .replace(' ', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_bad_chars() {
        assert_eq!(sanitize_name("My Project!"), "My_Project");
        assert_eq!(sanitize_name("hi/there"), "hithere");
        assert_eq!(sanitize_name("  spaces  "), "spaces");
        assert_eq!(sanitize_name("ok-name_1"), "ok-name_1");
    }
}
