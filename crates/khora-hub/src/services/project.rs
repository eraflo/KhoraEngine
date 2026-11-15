// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Project template creation for new Khora Engine projects.

use crate::services::git;
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Minimal project descriptor persisted as `project.json` in the root of the project.
#[derive(Serialize)]
struct ProjectDescriptor<'a> {
    name: &'a str,
    /// The game's version, which a built game and what the engine learns of
    /// it are filed under.
    version: &'a str,
    engine_version: &'a str,
    created_at: u64,
}

/// The version a new project's game starts at.
const FIRST_GAME_VERSION: &str = "0.1.0";

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
/// `src/` holds native Rust compiled into the game binary; `assets/scripts/`
/// holds the Ergon gameplay scripts, data the engine compiles and hot-reloads.
/// Git is initialised as `git` asks; a Git failure is logged, not returned.
/// Returns the project root, `<parent>/<name>` with the name sanitised.
///
/// # Errors
///
/// Fails when the sanitised name is empty, the folder already exists, or a
/// file cannot be written.
pub fn create_project(
    name: &str,
    parent: &Path,
    engine_version: &str,
    git: &GitInit,
) -> Result<PathBuf> {
    // Strips path separators, so the project cannot land outside `parent`.
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

    std::fs::create_dir_all(&root)
        .with_context(|| format!("Failed to create project directory '{}'", root.display()))?;

    for sub in &[
        "scenes", "textures", "meshes", "audio", "shaders", "scripts",
    ] {
        std::fs::create_dir_all(root.join("assets").join(sub))
            .with_context(|| format!("Failed to create assets/{} directory", sub))?;
    }

    std::fs::create_dir_all(root.join("src")).context("Failed to create src/ directory")?;

    std::fs::write(root.join(".gitignore"), default_gitignore())
        .context("Failed to write .gitignore")?;

    std::fs::write(root.join("README.md"), default_readme(name, engine_version))
        .context("Failed to write README.md")?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let descriptor = ProjectDescriptor {
        name,
        version: FIRST_GAME_VERSION,
        engine_version,
        created_at: now,
    };

    let json = serde_json::to_string_pretty(&descriptor)
        .context("Failed to serialize project descriptor")?;

    std::fs::write(root.join("project.json"), json).context("Failed to write project.json")?;

    // A real Ergon module, not a placeholder: the engine compiles every `.erg`
    // under `assets/scripts` at startup, so a fresh project's gameplay runs.
    // It is a script asset like any other, shown in the asset browser.
    std::fs::write(
        root.join("assets").join("scripts").join("main.erg"),
        default_main_erg(name),
    )
    .context("Failed to write assets/scripts/main.erg")?;

    // The editor creates the default scene (`assets/scenes/default.kscene`)
    // on first open when it is missing.

    // Git is best-effort: the project exists whether or not it succeeds.
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

/// Returns the Ergon behaviour a new project starts with: a module that compiles and runs.
///
/// It does nothing visible: `OnSpawn` fires once per entity carrying it, and a
/// fresh project has none, so its log line appears once the author attaches it.
fn default_main_erg(project_name: &str) -> String {
    format!(
        "// {project_name} — gameplay in Ergon.\n\
         //\n\
         // Every `.erg` under `assets/scripts` is compiled when the game starts,\n\
         // and recompiled when you save. A behaviour is a component: attach\n\
         // `Main` to an entity by adding its component, `Main`.\n\
         //\n\
         // A behaviour cannot write the world directly: `SetPosition`, `Despawn`\n\
         // and the rest queue a command the engine applies at a boundary where\n\
         // mutation is legal. That is what lets gameplay run beside the renderer\n\
         // rather than after it.\n\
         \n\
         behavior Main {{\n\
         \x20   // Fields are authored per entity: the inspector edits them, the\n\
         \x20   // scene file keeps them, and renaming one leaves the others alone.\n\
         \x20   float speed = 1.0;\n\
         \n\
         \x20   // Once per entity, the first time this behaviour runs on it.\n\
         \x20   void OnSpawn() {{\n\
         \x20       Log(\"{project_name}: Main is running\");\n\
         \x20   }}\n\
         \n\
         \x20   // Once per frame, if the frame budget allows. Out of budget, this\n\
         \x20   // is deferred whole rather than cut short — a half-run decision is\n\
         \x20   // worse than a late one.\n\
         \x20   void Update(float dt) {{\n\
         \x20   }}\n\
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

/// Returns whether the project root holds a `Cargo.toml`, the opt-in to native Rust.
///
/// Build Game then runs `cargo build` instead of stamping `khora-runtime`. The
/// file's presence is the whole contract: it is not parsed.
pub fn has_native_code(project_root: &Path) -> bool {
    project_root.join("Cargo.toml").is_file()
}

/// Scaffolds a native-Rust project on top of an existing Khora project.
///
/// Writes a `Cargo.toml` depending on `khora-sdk` at `engine_version` and a
/// `src/main.rs` calling `khora_sdk::run_default()`, as `khora-runtime` does,
/// and adds `target/` to the `.gitignore`. Native Rust is opt-in, behind
/// "Add Native Code". See the book: reference/editor.md, "Why two strategies".
///
/// # Errors
///
/// Fails when the project already has a `Cargo.toml` (see [`has_native_code`])
/// or a file cannot be written.
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

    // Keeps the project git-clean once the user runs `cargo build`.
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
    // Cargo's style for a package name: lowercase words joined by hyphens.
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
// To add components, or families of lanes (`register_agent!`), replace the
// body with your own `EngineApp + PhaseProvider` implementation and call
// `khora_sdk::run_winit::<...>(...)` directly. See the SDK docs for the
// full API.

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
///
/// # Errors
///
/// Fails when `editor_binary` is not a file, saying how to get one, or when
/// the process cannot start.
pub fn launch_editor(editor_binary: &str, project_path: &Path) -> Result<()> {
    if !Path::new(editor_binary).is_file() {
        anyhow::bail!(
            "no editor at '{editor_binary}' — build it with `cargo build -p khora-editor`, or reinstall this engine"
        );
    }
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
///
/// # Errors
///
/// Fails when the system has no way to open the folder.
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
    /// **A new project starts with a file that compiles.**
    ///
    /// A seed that does not compile fails at the first launch, and the author
    /// cannot tell their mistake from ours.
    #[test]
    fn the_seeded_behaviour_compiles() {
        let loader =
            khora_script::MemoryLoader::new().with("main.erg", super::default_main_erg("Demo"));
        let outcome = khora_script::compile_module(&loader, "main.erg");

        assert!(
            outcome.diagnostics.is_empty(),
            "seed does not compile: {:?}",
            outcome.diagnostics
        );
        assert!(outcome.program.is_some(), "seed produced no program");
    }

    /// The project's name reaches the file, so the first log line names it.
    #[test]
    fn the_seed_carries_the_project_name() {
        assert!(super::default_main_erg("Nimbus").contains("Nimbus"));
    }

    use super::*;

    /// **A new project names its version.**
    ///
    /// Its `project.json` holds the game's `version`, `0.1.0`, beside its name
    /// and the engine version it targets.
    #[test]
    fn a_new_project_names_its_version() -> Result<()> {
        let parent = std::env::temp_dir().join(format!(
            "khora-hub-new-project-version-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent)?;

        let written = create_project("Lantern", &parent, "0.3.0", &GitInit::None)
            .and_then(|root| Ok(std::fs::read_to_string(root.join("project.json"))?));
        let _ = std::fs::remove_dir_all(&parent);

        let project: serde_json::Value = serde_json::from_str(&written?)?;
        assert_eq!(project["version"], "0.1.0", "{project}");
        assert_eq!(project["name"], "Lantern", "{project}");
        assert_eq!(project["engine_version"], "0.3.0", "{project}");
        Ok(())
    }

    #[test]
    fn sanitize_strips_bad_chars() {
        assert_eq!(sanitize_name("My Project!"), "My_Project");
        assert_eq!(sanitize_name("hi/there"), "hithere");
        assert_eq!(sanitize_name("  spaces  "), "spaces");
        assert_eq!(sanitize_name("ok-name_1"), "ok-name_1");
    }

    /// **A missing editor is named, with how to get one.**
    ///
    /// After `target/` is cleaned, the dev engine's editor is simply not built.
    #[test]
    fn launching_a_missing_editor_says_how_to_build_it() {
        let missing = std::env::temp_dir().join("khora-no-such-editor.exe");
        let error = super::launch_editor(&missing.to_string_lossy(), std::path::Path::new("."))
            .expect_err("there is no editor to launch");
        let said = format!("{error:#}");
        assert!(said.contains("no editor at"), "{said}");
        assert!(said.contains("cargo build -p khora-editor"), "{said}");
    }
}
