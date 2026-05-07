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

    // ── assets/scripts/main.kscript ────────────────────────────────────
    // Seed an empty gameplay-scripts file so contributors have a starting
    // point. v1 has no scripting language yet — this is a JSON stub that
    // documents where game logic will live once the scripting runtime
    // lands. The file is treated as a regular project asset (the canonical
    // ext→type map registers `.kscript` under the `script` slot), so it
    // shows up in the asset browser like any other resource.
    std::fs::write(
        root.join("assets").join("scripts").join("main.kscript"),
        default_main_kscript(name),
    )
    .context("Failed to write assets/scripts/main.kscript")?;

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

fn default_main_kscript(project_name: &str) -> String {
    // Stub: structured JSON that the future Khora script runtime will
    // interpret. For now it's documentation. The fields chosen here are
    // forward-compatible with the planned schema:
    //   - `entry`: which behaviour table to execute first
    //   - `behaviours`: map of named behaviour blocks
    //   - `bindings`: which entities/components the behaviour reaches
    format!(
        "{{\n  \
            \"_comment\": \"Khora gameplay scripts — data-driven, hot-reloadable. \
            The scripting runtime is not implemented yet (v0.x); this file is \
            a placeholder authored by the hub when a project is created. Edit \
            it once the runtime is available.\",\n  \
            \"project\": \"{project_name}\",\n  \
            \"version\": 1,\n  \
            \"entry\": \"main\",\n  \
            \"behaviours\": {{\n    \
                \"main\": {{\n      \
                    \"on_start\": [],\n      \
                    \"on_tick\": []\n    \
                }}\n  \
            }}\n\
        }}\n",
        project_name = project_name
    )
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

/// Returns `true` when the project root already holds a `Cargo.toml` —
/// indicating the user has opted into native-Rust mode (Build Game will
/// then invoke `cargo build` instead of stamping `khora-runtime`).
///
/// The check is purely "file exists" — no parsing — to keep the rule
/// simple: presence of the file IS the contract.
pub fn has_native_code(project_root: &Path) -> bool {
    project_root.join("Cargo.toml").is_file()
}

/// Scaffolds a native-Rust project on top of an existing Khora project.
///
/// Writes:
/// - `<root>/Cargo.toml` — minimal manifest depending on `khora-sdk` for the
///   project's engine version.
/// - `<root>/src/main.rs` — calls `khora_sdk::run_default()` so a freshly
///   scaffolded project is functionally identical to the pre-built
///   `khora-runtime`. Users edit this file when they want to register
///   custom components / agents / lanes.
/// - `<root>/src/lib.rs` is **not** generated — the user adds it themselves
///   if they want a library split.
///
/// Idempotent: returns an error if `Cargo.toml` already exists (the caller
/// should pre-check via [`has_native_code`] and present a clear UI).
///
/// **Why this is opt-in.** Khora's philosophy treats `src/` and
/// `assets/scripts/` as two distinct first-class roles, neither being the
/// default. Auto-generating `Cargo.toml` at project creation would push
/// every user into the native-Rust flow, which most don't need. Build Game
/// uses the `khora-runtime` stamp by default; the user explicitly upgrades
/// to native Rust by clicking "Add Native Code" when they need it.
pub fn add_native_code(
    project_root: &Path,
    project_name: &str,
    engine_version: &str,
) -> Result<()> {
    if has_native_code(project_root) {
        anyhow::bail!(
            "Project at '{}' already has a Cargo.toml — native code is already enabled",
            project_root.display()
        );
    }
    let pkg_name = native_pkg_name(project_name);

    let cargo_toml = native_cargo_toml(&pkg_name, engine_version);
    std::fs::write(project_root.join("Cargo.toml"), cargo_toml)
        .context("Failed to write Cargo.toml")?;

    let src_dir = project_root.join("src");
    std::fs::create_dir_all(&src_dir).context("Failed to create src/ directory")?;
    std::fs::write(src_dir.join("main.rs"), native_main_rs())
        .context("Failed to write src/main.rs")?;

    // Append target/ to .gitignore if not already there. Keeps the project
    // git-clean after the user runs cargo build.
    let gitignore_path = project_root.join(".gitignore");
    if let Ok(existing) = std::fs::read_to_string(&gitignore_path)
        && !existing.split_whitespace().any(|line| line == "target/")
    {
        let mut updated = existing;
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str("\n# Native-Rust build output\ntarget/\n");
        std::fs::write(&gitignore_path, updated)
            .context("Failed to update .gitignore for target/")?;
    }

    Ok(())
}

fn native_pkg_name(project_name: &str) -> String {
    // Cargo package names are kebab/snake-friendly identifiers. We
    // sanitise like the project-name sanitiser but force lowercase and
    // map underscores to hyphens for the canonical Cargo style.
    let cleaned: String = project_name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed: String = cleaned.trim_matches('-').to_owned();
    if trimmed.is_empty() {
        "khora-game".to_owned()
    } else {
        trimmed
    }
}

fn native_cargo_toml(pkg_name: &str, engine_version: &str) -> String {
    format!(
        r#"# Generated by Khora Hub at "Add Native Code". Edit freely — the hub
# only seeds this file once and never rewrites it.
#
# `khora-sdk` is the only Khora dependency you need; it re-exports
# everything else (ECS, math, asset types, the run_default() entry-point).
[package]
name = "{pkg_name}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{pkg_name}"
path = "src/main.rs"

[dependencies]
khora-sdk = "{engine_version}"
anyhow = "1"
log = "0.4"
env_logger = "0.11"

[profile.release]
lto = true
codegen-units = 1
"#,
        pkg_name = pkg_name,
        engine_version = engine_version,
    )
}

fn native_main_rs() -> &'static str {
    r#"// Generated by Khora Hub at "Add Native Code".
//
// This file is the entry point for your game's native-Rust binary. By
// default it just delegates to `khora_sdk::run_default()`, which is the
// same logic the pre-built `khora-runtime` uses (auto-detect packed/loose
// assets, register every default decoder, load the scene named in
// `runtime.json`, and tick).
//
// To register custom components / agents / lanes, replace the body with
// your own `EngineApp + AgentProvider + PhaseProvider` implementation and
// call `khora_sdk::run_winit::<...>(...)` directly. See the SDK docs for
// the full API.

use anyhow::Result;
use khora_sdk::prelude::*;

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

fn main() -> Result<()> {
    use env_logger::{Builder, Env};
    Builder::from_env(Env::default().default_filter_or("info"))
        .filter_module("wgpu_hal::vulkan::instance", log::LevelFilter::Off)
        .init();

    khora_sdk::run_default()
}
"#
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
