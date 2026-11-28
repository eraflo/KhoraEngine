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
use khora_sdk::trust::{FileGuard, ProjectFile, ReadError, Refusals, Trust};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The version a new project's game starts at.
const FIRST_GAME_VERSION: &str = "0.1.0";

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
///   assets/sounds/
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
        "scenes", "textures", "meshes", "sounds", "shaders", "scripts",
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

    std::fs::write(root.join(ProjectFile::Manifest.rel_path()), json)
        .context("Failed to write project.json")?;

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
     # Locks held while the editor writes the identity registry or a build\n\
     .khora/*.lock\n\
     dist/*.lock\n\
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
pub(crate) fn has_native_code(project_root: &Path) -> bool {
    project_root.join("Cargo.toml").is_file()
}

/// Scaffolds a native-Rust project on top of an existing Khora project.
///
/// Writes a `Cargo.toml` depending on `khora-sdk` at `engine_version` and a
/// `src/main.rs` calling `khora_sdk::run_default()`, as `khora-runtime` does,
/// and adds `target/` to the `.gitignore`. Native Rust is opt-in, behind
/// "Add Native Code". See the book: reference/editor.md, "Why two strategies".
///
/// Whole or not at all: every file is read first (the `.gitignore` through
/// [`project_guard`]), then written, the `Cargo.toml` last; a write that fails
/// removes what this call made and gives the `.gitignore` back its bytes.
///
/// # Errors
///
/// Fails when the project already has a `Cargo.toml` (see [`has_native_code`])
/// or a `src/main.rs`, when its `.gitignore` is refused or unreadable, or when
/// a file cannot be written.
pub(crate) fn add_native_code(
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
    let src_dir = project_root.join("src");
    let main_rs = src_dir.join("main.rs");
    if main_rs.exists() {
        anyhow::bail!(
            "Project at '{}' already has a src/main.rs, which native code would write over",
            project_root.display()
        );
    }
    // Keeps the project git-clean once the user runs `cargo build`.
    let gitignore_path = project_root.join(ProjectFile::GitIgnore.rel_path());
    let existing = match project_guard(project_root).read_project_file(ProjectFile::GitIgnore) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.is_missing() => None,
        Err(error) => {
            return Err(anyhow::anyhow!(
                "the project's .gitignore is not read, so native code is not added: {error}"
            ));
        }
    };
    let gitignore = existing
        .as_deref()
        .and_then(|bytes| gitignore_with_target(&String::from_utf8_lossy(bytes)));
    let cargo_toml = native_cargo_toml(&native_pkg_name(project_name), engine_version);

    // What this call made, to remove should a later write fail.
    let mut made: Vec<PathBuf> = Vec::new();
    let mut gitignore_written = false;
    let written = (|| -> Result<()> {
        if !src_dir.is_dir() {
            std::fs::create_dir(&src_dir).context("Failed to create src/ directory")?;
            made.push(src_dir.clone());
        }
        khora_sdk::fs::write_new(&main_rs, native_main_rs().as_bytes())
            .context("Failed to write src/main.rs")?;
        made.push(main_rs.clone());
        if let Some(updated) = &gitignore {
            khora_sdk::fs::write_atomic(&gitignore_path, updated.as_bytes())
                .context("Failed to update .gitignore for target/")?;
            gitignore_written = true;
        }
        khora_sdk::fs::write_new(&project_root.join("Cargo.toml"), cargo_toml.as_bytes())
            .context("Failed to write Cargo.toml")?;
        Ok(())
    })();
    if written.is_err() {
        if let (true, Some(previous)) = (gitignore_written, &existing)
            && let Err(error) = khora_sdk::fs::write_atomic(&gitignore_path, previous)
        {
            log::warn!("the .gitignore could not be given back its bytes: {error}");
        }
        for path in made.iter().rev() {
            let removed = if path.is_dir() {
                std::fs::remove_dir(path)
            } else {
                std::fs::remove_file(path)
            };
            if let Err(error) = removed {
                log::warn!(
                    "{} is left by a failed Add Native Code: {error}",
                    path.display()
                );
            }
        }
    }
    written
}

/// Returns `existing`, a `.gitignore`'s text, with `target/` added; `None` when it already lists it.
fn gitignore_with_target(existing: &str) -> Option<String> {
    if existing.split_whitespace().any(|line| line == "target/") {
        return None;
    }
    let mut updated = existing.to_owned();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str("\n# Native-Rust build output\ntarget/\n");
    Some(updated)
}

/// Returns a guard reading the files of the project at `root`.
///
/// The hub runs nothing of a project and never trusts one: it reads under the
/// strict caps, its refusals kept in memory and returned to the caller.
pub(crate) fn project_guard(root: &Path) -> FileGuard {
    FileGuard::new(root, Trust::Restricted, Refusals::detached())
}

/// Reads the `project.json` of the project at `root` through [`project_guard`].
///
/// `None` when it has none; a file that does not parse reads as `null`.
///
/// # Errors
///
/// [`ReadError::Refused`] when the guard refuses it, over its cap say;
/// [`ReadError::Io`] when it exists but cannot be read.
pub(crate) fn read_project_json(root: &Path) -> Result<Option<serde_json::Value>, ReadError> {
    match project_guard(root).read_project_file(ProjectFile::Manifest) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).unwrap_or_default())),
        Err(error) if error.is_missing() => Ok(None),
        Err(error) => Err(error),
    }
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
pub(crate) fn reveal_in_explorer(path: &Path) -> Result<()> {
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
            .and_then(|root| Ok(read_project_json(&root)?));
        let _ = std::fs::remove_dir_all(&parent);

        let project = written?.expect("the new project has a `project.json`");
        assert_eq!(project["version"], "0.1.0", "{project}");
        assert_eq!(project["name"], "Lantern", "{project}");
        assert_eq!(project["engine_version"], "0.3.0", "{project}");
        Ok(())
    }

    /// **A new project's `.gitignore` keeps the lock files out of git.**
    ///
    /// The identity registry's lock under `.khora/` and a build's under
    /// `dist/` are held while the editor writes, and never shared.
    #[test]
    fn a_new_projects_gitignore_leaves_out_the_lock_files() -> Result<()> {
        let parent = std::env::temp_dir().join(format!(
            "khora-hub-new-project-gitignore-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&parent)?;

        let written = create_project("Lantern", &parent, "0.3.0", &GitInit::None)
            .and_then(|root| Ok(std::fs::read_to_string(root.join(".gitignore"))?));
        let _ = std::fs::remove_dir_all(&parent);

        let gitignore = written?;
        for pattern in [".khora/*.lock", "dist/*.lock"] {
            assert!(
                gitignore.lines().any(|line| line == pattern),
                "`{pattern}` is not ignored:\n{gitignore}"
            );
        }
        Ok(())
    }

    /// **The hub reads a project's files under the strict caps**: it never
    /// trusts a project, so a `project.json` over the strict cap is refused.
    #[test]
    fn the_hub_reads_a_projects_files_under_the_strict_caps() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("khora-hub-strict-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        std::fs::File::create(root.join("project.json"))?.set_len((256 << 10) + 1)?;
        let missing = read_project_json(&root.join("none"));
        let oversized = read_project_json(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert!(matches!(missing, Ok(None)), "{missing:?}");
        assert!(
            matches!(oversized, Err(ReadError::Refused(_))),
            "{oversized:?}"
        );
        Ok(())
    }

    /// **Native code is added whole or not at all.**
    ///
    /// A `.gitignore` the hub refuses fails the step before anything is written:
    /// a project left with a `Cargo.toml` would refuse the retry as done.
    #[test]
    fn native_code_is_added_whole_or_not_at_all() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("khora-hub-native-whole-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        std::fs::File::create(root.join(".gitignore"))?.set_len((256 << 10) + 1)?;

        let added = add_native_code(&root, "Lantern", "0.3.0");
        let half_done = has_native_code(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert!(added.is_err(), "the .gitignore is refused");
        assert!(!half_done, "a Cargo.toml was written by a step that failed");
        Ok(())
    }

    /// **A write failing midway leaves nothing native code wrote.**
    ///
    /// A `src` that is a file fails the scaffold after its first steps; a
    /// `Cargo.toml` that is a folder fails its last write. Either way no file of
    /// it is left, and the `.gitignore` reads as it did.
    #[test]
    fn a_write_failing_midway_leaves_nothing_native_code_wrote() -> Result<()> {
        for (tag, blocker) in [("src-file", "src"), ("cargo-folder", "Cargo.toml")] {
            let root = std::env::temp_dir().join(format!(
                "khora-hub-native-midway-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root)?;
            std::fs::write(root.join(".gitignore"), "build/\n")?;
            if blocker == "src" {
                std::fs::write(root.join("src"), b"a file")?;
            } else {
                std::fs::create_dir_all(root.join("src"))?;
                std::fs::create_dir_all(root.join("Cargo.toml"))?;
            }

            let added = add_native_code(&root, "Lantern", "0.3.0");
            let cargo_written = root.join("Cargo.toml").is_file();
            let main_written = root.join("src").join("main.rs").exists();
            let gitignore = std::fs::read_to_string(root.join(".gitignore"));
            let _ = std::fs::remove_dir_all(&root);

            assert!(added.is_err(), "{tag}: the scaffold fails");
            assert!(!cargo_written, "{tag}: a Cargo.toml is left");
            assert!(!main_written, "{tag}: a src/main.rs is left");
            assert_eq!(gitignore?, "build/\n", "{tag}: the .gitignore changed");
        }
        Ok(())
    }

    /// **Native code is added whole**: the manifest, the entry point and
    /// `target/` in the `.gitignore`; a `src/main.rs` already there is never written over.
    #[test]
    fn native_code_is_added_whole_and_never_over_a_main_rs() -> Result<()> {
        let root =
            std::env::temp_dir().join(format!("khora-hub-native-added-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src"))?;
        std::fs::write(root.join(".gitignore"), "build/")?;

        let added = add_native_code(&root, "Lantern", "0.3.0");
        let gitignore = std::fs::read_to_string(root.join(".gitignore"));
        let scaffolded = has_native_code(&root) && root.join("src").join("main.rs").is_file();
        std::fs::remove_file(root.join("Cargo.toml"))?;
        let again = add_native_code(&root, "Lantern", "0.3.0");
        let main_kept = std::fs::read_to_string(root.join("src").join("main.rs"));
        let _ = std::fs::remove_dir_all(&root);

        added?;
        assert!(scaffolded, "the manifest and the entry point are written");
        assert!(gitignore?.lines().any(|line| line == "target/"));
        assert!(again.is_err(), "a src/main.rs already there is refused");
        assert_eq!(main_kept?, native_main_rs());
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
