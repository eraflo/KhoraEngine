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

use crate::helpers::*;
use anyhow::Result;
use std::time::Instant;

pub fn build() -> Result<()> {
    print_task_start("Building All Crates", HAMMER, BLUE);
    println!(
        "{}💡 Info:{} Compiling all workspace crates in debug mode",
        BOLD, RESET
    );
    execute_command(
        "cargo",
        &["build", "--workspace", "--exclude", "xtask"],
        "Build",
    )?;
    Ok(())
}

pub fn test() -> Result<()> {
    print_task_start("Running All Tests", TEST_TUBE, GREEN);
    println!(
        "{}💡 Info:{} Running unit tests, integration tests and doc tests",
        BOLD, RESET
    );
    execute_command("cargo", &["test", "--workspace"], "Tests")?;
    Ok(())
}

pub fn check() -> Result<()> {
    print_task_start("Checking All Crates", MAGNIFIER, CYAN);
    println!(
        "{}💡 Info:{} Checking code for errors without building executables",
        BOLD, RESET
    );
    execute_command("cargo", &["check", "--workspace"], "Check")?;
    Ok(())
}

pub fn format() -> Result<()> {
    print_task_start("Formatting Code", BRUSH, MAGENTA);
    println!(
        "{}💡 Info:{} Formatting code using rustfmt with default settings",
        BOLD, RESET
    );
    // Note: `fmt` requires `--all` not `--workspace`
    execute_command("cargo", &["fmt", "--all"], "Format")?;
    Ok(())
}

pub fn clippy() -> Result<()> {
    print_task_start("Running Clippy", CLIPPY, YELLOW);
    println!(
        "{}💡 Info:{} Running Clippy linter with warnings as errors",
        BOLD, RESET
    );
    execute_command(
        "cargo",
        &["clippy", "--workspace", "--", "-D", "warnings"],
        "Clippy",
    )?;
    Ok(())
}

pub fn all() -> Result<()> {
    println!("{}", crate::helpers::BANNER);
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
