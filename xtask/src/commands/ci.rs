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

use crate::exec::*;
use crate::term::*;
use anyhow::Result;
use std::time::Instant;

// Every command selects the same packages, targets and features: Cargo keeps
// one build per combination and shares nothing between them, so mixing flag
// sets recompiles the crates they touch.
const SCOPE: [&str; 3] = ["--workspace", "--all-targets", "--all-features"];

// The benches run as smoke tests after cargo's default test targets. Named by
// `--bench *`: `--benches` would take every `bench = true` target, libs and bins
// included, and run their tests twice. See the book:
// contributing/setup.md, "Keeping `target/` in check".
const BENCHES: [&str; 2] = ["--bench", "*"];

/// Builds every workspace crate, all targets and features.
pub fn build() -> Result<()> {
    print_task_start("Building All Crates", HAMMER, BLUE);
    println!(
        "{}💡 Info:{} Compiling all workspace crates with all features and targets",
        BOLD, RESET
    );
    execute_command("cargo", &["build", SCOPE[0], SCOPE[1], SCOPE[2]], "Build")?;
    Ok(())
}

/// Runs the workspace tests with nextest, the benches as smoke tests, then `xtask`'s tests.
pub fn test() -> Result<()> {
    print_task_start("Running All Tests", TEST_TUBE, GREEN);
    println!(
        "{}💡 Info:{} Running tests using cargo-nextest with all features, then the benches as smoke tests",
        BOLD, RESET
    );
    // `xtask` is excluded from the workspace pass and tested on its own: on
    // Windows a running executable cannot be overwritten, and the workspace
    // build would relink `target/debug/xtask.exe` while it runs this gate.
    execute_command(
        "cargo",
        &[
            "nextest",
            "run",
            SCOPE[0],
            "--exclude",
            "xtask",
            SCOPE[2],
            "--locked",
        ],
        "Tests",
    )?;
    execute_command(
        "cargo",
        &[
            "nextest",
            "run",
            SCOPE[0],
            "--exclude",
            "xtask",
            BENCHES[0],
            BENCHES[1],
            SCOPE[2],
            "--locked",
        ],
        "Bench smoke tests",
    )?;
    execute_command(
        "cargo",
        &["nextest", "run", "-p", "xtask", SCOPE[2], "--locked"],
        "xtask tests",
    )?;
    Ok(())
}

/// Runs `cargo check` on every workspace crate, all targets and features.
pub fn check() -> Result<()> {
    print_task_start("Checking All Crates", MAGNIFIER, CYAN);
    println!(
        "{}💡 Info:{} Checking code for errors without building executables",
        BOLD, RESET
    );
    execute_command("cargo", &["check", SCOPE[0], SCOPE[1], SCOPE[2]], "Check")?;
    Ok(())
}

/// Checks the formatting of the whole workspace.
pub fn format() -> Result<()> {
    print_task_start("Formatting Code", BRUSH, MAGENTA);
    println!(
        "{}💡 Info:{} Checking code formatting (use 'cargo fmt --all' to auto-fix)",
        BOLD, RESET
    );
    // Checks only, as CI does: a gate never rewrites files.
    execute_command("cargo", &["fmt", "--all", "--", "--check"], "Format")?;
    Ok(())
}

/// Checks that the workspace-hack unifies every third-party dependency's features.
///
/// See the book: contributing/setup.md, "Keeping `target/` in check".
///
/// # Errors
///
/// Fails when a dependency changed without `cargo hakari generate` and
/// `cargo hakari manage-deps`: run them, and commit what they change.
pub fn hakari() -> Result<()> {
    print_task_start("Checking the feature unification", MAGNIFIER, CYAN);
    execute_command(
        "cargo",
        &["hakari", "generate", "--diff"],
        "Hakari (workspace-hack contents)",
    )?;
    execute_command(
        "cargo",
        &["hakari", "manage-deps", "--dry-run"],
        "Hakari (workspace-hack dependencies)",
    )?;
    Ok(())
}

/// Runs clippy on every workspace crate, warnings denied.
pub fn clippy() -> Result<()> {
    print_task_start("Running Clippy", CLIPPY, YELLOW);
    println!(
        "{}💡 Info:{} Running Clippy linter with all features and warnings as errors",
        BOLD, RESET
    );
    execute_command(
        "cargo",
        &[
            "clippy", SCOPE[0], SCOPE[1], SCOPE[2], "--", "-D", "warnings",
        ],
        "Clippy",
    )?;
    Ok(())
}

/// Builds every workspace crate's docs, private items included, warnings denied.
///
/// Checks the rustdoc lints of `[workspace.lints.rustdoc]`, which clippy does not run:
/// a broken or private intra-doc link, a bare URL, a stray HTML tag.
///
/// # Errors
///
/// Fails when rustdoc reports any warning, or the command cannot be run.
pub fn doc() -> Result<()> {
    print_task_start("Building the Docs", MAGNIFIER, BLUE);
    println!(
        "{}💡 Info:{} Building the docs of every crate, private items included, warnings as errors",
        BOLD, RESET
    );
    execute_command_with_env(
        "cargo",
        &[
            "doc",
            SCOPE[0],
            "--no-deps",
            "--document-private-items",
            SCOPE[2],
            "--locked",
        ],
        &[("RUSTDOCFLAGS", "-D warnings")],
        "Docs",
    )?;
    Ok(())
}

/// Runs the doc tests, which nextest does not run.
pub fn doc_test() -> Result<()> {
    print_task_start("Running Doc Tests", TEST_TUBE, GREEN);
    execute_command(
        "cargo",
        &["test", SCOPE[0], "--doc", SCOPE[2], "--locked"],
        "Doc tests",
    )?;
    Ok(())
}

/// Runs what CI checks, fastest failure first: format, feature unification, clippy, docs, tests, doc tests.
///
/// Run it once a change is done; while iterating, `cargo t -p <crate> [filter]`.
///
/// # Errors
///
/// Fails at the first check that fails, and runs none after it.
pub fn gate() -> Result<()> {
    let start_time = Instant::now();
    format()?;
    hakari()?;
    clippy()?;
    doc()?;
    test()?;
    doc_test()?;
    println!(
        "\n{}{}{} Gate passed in {:.0}s{}",
        BOLD,
        GREEN,
        CHECK,
        start_time.elapsed().as_secs_f64(),
        RESET
    );
    Ok(())
}

/// Runs build, test, check, format and clippy in turn, each even when one before failed.
///
/// # Errors
///
/// Fails when any of them failed.
pub fn all() -> Result<()> {
    println!("{}", crate::term::BANNER);
    println!("{}{}Starting full build pipeline...{}", BOLD, CYAN, RESET);
    println!(
        "{}💡 Pipeline:{} This will run build → test → check → format → clippy",
        BOLD, RESET
    );

    let start_time = Instant::now();
    let tasks = [
        ("Build Phase", build as fn() -> Result<()>),
        ("Test Phase", test),
        ("Check Phase", check),
        ("Format Phase", format),
        ("Clippy Phase", clippy),
    ];
    let total_tasks = tasks.len();
    let mut success_count = 0;

    for (i, (name, task_fn)) in tasks.iter().enumerate() {
        println!(
            "\n{}{}[{}/{}] {}{}",
            BOLD,
            BLUE,
            i + 1,
            total_tasks,
            name,
            RESET
        );
        if task_fn().is_ok() {
            success_count += 1;
        }
    }

    let total_duration = start_time.elapsed();
    println!(
        "\n{}{}╔═══════════════════════════════════════╗{}",
        BOLD, CYAN, RESET
    );
    println!(
        "{}{}║            PIPELINE SUMMARY           ║{}",
        BOLD, CYAN, RESET
    );
    println!(
        "{}{}╚═══════════════════════════════════════╝{}",
        BOLD, CYAN, RESET
    );

    if success_count == total_tasks {
        println!(
            "{}{} {} All {} tasks completed successfully! {}{}",
            BOLD, GREEN, CHECK, total_tasks, ROCKET, RESET
        );
    } else {
        println!(
            "{}{} ⚠ {}/{} tasks completed{}",
            BOLD, YELLOW, success_count, total_tasks, RESET
        );
    }

    println!(
        "{}{}Total time: {:.2}s{}",
        BOLD,
        BLUE,
        total_duration.as_secs_f64(),
        RESET
    );

    if success_count != total_tasks {
        anyhow::bail!(
            "Pipeline failed with {}/{} successful tasks.",
            success_count,
            total_tasks
        );
    }

    Ok(())
}
