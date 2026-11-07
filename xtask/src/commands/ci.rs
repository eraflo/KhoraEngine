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

// The tests are built from cargo's default targets — which leave out a
// target marked `test = false` — and then from the benches, whose functions
// run as smoke tests. Each test target is a binary linking the whole engine,
// so one holding no test costs a full link for nothing: such a target sets
// `test = false` in its manifest (an `xtask` check keeps a test from hiding
// there). Naming `--lib --bins --tests` instead would build those targets
// anyway, since an explicit selection overrides `test = false`. The benches
// are named by `--bench *`: `--benches` would take every target with
// `bench = true`, libs and bins included, and run their tests a second time.
const BENCHES: [&str; 2] = ["--bench", "*"];

pub fn build() -> Result<()> {
    print_task_start("Building All Crates", HAMMER, BLUE);
    println!(
        "{}💡 Info:{} Compiling all workspace crates with all features and targets",
        BOLD, RESET
    );
    execute_command("cargo", &["build", SCOPE[0], SCOPE[1], SCOPE[2]], "Build")?;
    Ok(())
}

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

pub fn check() -> Result<()> {
    print_task_start("Checking All Crates", MAGNIFIER, CYAN);
    println!(
        "{}💡 Info:{} Checking code for errors without building executables",
        BOLD, RESET
    );
    execute_command("cargo", &["check", SCOPE[0], SCOPE[1], SCOPE[2]], "Check")?;
    Ok(())
}

pub fn format() -> Result<()> {
    print_task_start("Formatting Code", BRUSH, MAGENTA);
    println!(
        "{}💡 Info:{} Checking code formatting (use 'cargo fmt --all' to auto-fix)",
        BOLD, RESET
    );
    // Match CI: check formatting without modifying files
    execute_command("cargo", &["fmt", "--all", "--", "--check"], "Format")?;
    Ok(())
}

/// The workspace-hack is up to date: every third-party dependency builds
/// with one feature set, whichever crate a command builds (RULES §2b). Fails
/// when a dependency changed without `cargo hakari generate` and `cargo hakari
/// manage-deps` — run them, and commit what they change.
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

/// Doc tests: nextest does not run them.
pub fn doc_test() -> Result<()> {
    print_task_start("Running Doc Tests", TEST_TUBE, GREEN);
    execute_command(
        "cargo",
        &["test", SCOPE[0], "--doc", SCOPE[2], "--locked"],
        "Doc tests",
    )?;
    Ok(())
}

/// What CI checks, in the order that fails fastest: format, the feature
/// unification, clippy, tests, doc tests. Stops at the first failure. Run it once, when a change is done;
/// while iterating, `cargo t -p <crate> [filter]`.
pub fn gate() -> Result<()> {
    let start_time = Instant::now();
    format()?;
    hakari()?;
    clippy()?;
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
