// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Thin wrapper around the system `git` CLI.
//!
//! Shells out to `git` rather than pulling in `git2` to keep the hub binary
//! small and to inherit the user's existing credential helper / SSH setup.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

/// Runs `git <args...>` in `cwd`. Returns stdout on success.
pub fn run_git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to spawn `git {}`", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Returns true if `git` is reachable on PATH.
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `git init -b main` + identity fallback + initial commit of all files.
///
/// `author` is used as a fallback identity if `user.name` / `user.email` are
/// not configured globally — we never overwrite an existing global config.
pub fn init_with_initial_commit(repo: &Path, author_name: &str, author_email: &str) -> Result<()> {
    run_git(repo, &["init", "-b", "main"])?;

    // Set local identity only if global one is missing — avoid touching the
    // user's global config.
    if !has_local_identity(repo) {
        run_git(repo, &["config", "user.name", author_name])?;
        run_git(repo, &["config", "user.email", author_email])?;
    }

    run_git(repo, &["add", "."])?;
    run_git(repo, &["commit", "-m", "Initial commit"])?;
    Ok(())
}

fn has_local_identity(repo: &Path) -> bool {
    let name = run_git(repo, &["config", "--get", "user.name"]).is_ok();
    let email = run_git(repo, &["config", "--get", "user.email"]).is_ok();
    name && email
}

/// Adds `origin` and (optionally) pushes `main` upstream.
pub fn add_remote_and_push(repo: &Path, remote_url: &str, push: bool) -> Result<()> {
    run_git(repo, &["remote", "add", "origin", remote_url])?;
    if push {
        run_git(repo, &["push", "-u", "origin", "main"])?;
    }
    Ok(())
}
