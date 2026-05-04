// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Project template creation for new Khora Engine projects.

use crate::git;
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

/// Git initialization mode for new projects.
#[derive(Debug, Clone, Default)]
pub enum GitInit {
    /// Don't initialize a git repository at all.
    None,
    /// `git init` + initial commit only.
    #[default]
    Local,
    /// `git init` + initial commit + add `origin` (already-created repo on GitHub).
    /// If `push` is true, also `git push -u origin main`.
    LocalAndRemote { remote_url: String, push: bool },
}

/// Creates a new Khora Engine project on disk.
///
/// Directory layout:
/// ```text
/// <parent>/<name>/
///   project.json           ← project descriptor
///   src/                   ← native Rust extensions (compiled)
///   assets/                ← asset root (loaded at runtime)
///   assets/scenes/         ← default scene folder
///   assets/textures/
///   assets/meshes/
///   assets/audio/
///   assets/shaders/
///   assets/scripts/        ← gameplay scripts (data, hot-reloadable)
/// ```
///
/// `src/` is for native Rust extensions and custom components compiled into
/// the game binary. `assets/scripts/` is for gameplay scripts treated as
/// runtime data — eventually a custom scripting language for live editing.
///
/// Returns the absolute path to the project root directory.
pub fn create_project(
    name: &str,
    parent: &Path,
    engine_version: &str,
    git: &GitInit,
) -> Result<PathBuf> {
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

    for sub in &[
        "scenes", "textures", "meshes", "audio", "shaders", "scripts",
    ] {
        std::fs::create_dir_all(root.join("assets").join(sub))
            .with_context(|| format!("Failed to create assets/{} directory", sub))?;
    }

    // Create src/ directory for user game code.
    std::fs::create_dir_all(root.join("src")).context("Failed to create src/ directory")?;

    // Write .gitignore.
    std::fs::write(root.join(".gitignore"), default_gitignore())
        .context("Failed to write .gitignore")?;

    // Write README.md.
    std::fs::write(root.join("README.md"), default_readme(name, engine_version))
        .context("Failed to write README.md")?;

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

    // Note: The editor creates the default scene (assets/scenes/default.kscene)
    // on first open if it doesn't exist. No need to create it here.

    // ── Git initialization (best-effort: warn on failure, don't unwind) ──
    if !matches!(git, GitInit::None)
        && let Err(e) = init_git(&root, name, git)
    {
        log::warn!("Git initialization failed (non-fatal): {e}");
    }

    Ok(root)
}

fn init_git(root: &Path, project_name: &str, git: &GitInit) -> Result<()> {
    if !git::git_available() {
        anyhow::bail!("`git` not found on PATH — skipping repo initialization");
    }

    git::init_with_initial_commit(root, project_name, &format!("{project_name}@khora.local"))?;

    if let GitInit::LocalAndRemote { remote_url, push } = git {
        git::add_remote_and_push(root, remote_url, *push)?;
    }
    Ok(())
}

fn default_gitignore() -> &'static str {
    "# Build artifacts\n\
     target/\n\
     \n\
     # Editor / IDE\n\
     .idea/\n\
     .vscode/\n\
     *.iml\n\
     \n\
     # OS\n\
     .DS_Store\n\
     Thumbs.db\n\
     \n\
     # Misc\n\
     *.tmp\n\
     *.swp\n\
     "
}

fn default_readme(name: &str, engine_version: &str) -> String {
    format!(
        "# {name}\n\n\
         A Khora Engine project.\n\n\
         - **Engine version**: `{engine_version}`\n\n\
         ## Getting started\n\n\
         Open this folder from the Khora Hub or run the editor manually:\n\n\
         ```sh\n\
         khora-editor --project .\n\
         ```\n",
    )
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
