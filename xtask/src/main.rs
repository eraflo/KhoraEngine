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

// Build automation and scripting tasks for Khora Engine
// Run with: cargo xtask <command>

use std::process::Command;
use std::time::Instant;

// ANSI color codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const BLUE: &str = "\x1b[34m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";

// Visual symbols
const CHECK: &str = "✓";
const CROSS: &str = "✗";
const GEAR: &str = "⚙";
const ROCKET: &str = "🚀";
const HAMMER: &str = "🔨";
const TEST_TUBE: &str = "🧪";
const MAGNIFIER: &str = "🔍";
const BRUSH: &str = "🎨";
const CLIPPY: &str = "📎";

fn print_banner() {
    println!("{}{}", BOLD, CYAN);
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!(
        "║                    {} KHORA ENGINE {}                     ║",
        ROCKET, GEAR
    );
    println!("║                   Build Automation Tool                   ║");
    println!("╚═══════════════════════════════════════════════════════════╝");
    println!("{}", RESET);
}

fn print_help() {
    print_banner();
    println!("{}{}Usage:{} cargo xtask <command>\n", BOLD, YELLOW, RESET);
    println!("{}Available commands:{}", BOLD, RESET);
    println!(
        "  {} {} {}build{}   - Build all crates",
        HAMMER, BLUE, BOLD, RESET
    );
    println!(
        "  {} {} {}test{}    - Run all tests",
        TEST_TUBE, GREEN, BOLD, RESET
    );
    println!(
        "  {} {} {}check{}   - Run cargo check on all crates",
        MAGNIFIER, CYAN, BOLD, RESET
    );
    println!(
        "  {} {} {}format{}  - Format all code",
        BRUSH, MAGENTA, BOLD, RESET
    );
    println!(
        "  {} {} {}clippy{}  - Run clippy on all crates",
        CLIPPY, YELLOW, BOLD, RESET
    );
    println!(
        "  {} {} {}all{}     - Run all tasks",
        ROCKET, RED, BOLD, RESET
    );
}

fn print_task_start(task_name: &str, emoji: &str, color: &str) {
    println!(
        "\n{}{}━━━ {} {} {}━━━{}",
        BOLD, color, emoji, task_name, emoji, RESET
    );
}

fn print_success(message: &str) {
    println!("{}{} {} {}{}", BOLD, GREEN, CHECK, message, RESET);
}

fn print_error(message: &str) {
    println!("{}{} {} {}{}", BOLD, RED, CROSS, message, RESET);
}

fn print_command_info(cmd: &str, args: &[&str]) {
    let full_command = format!("{} {}", cmd, args.join(" "));
    println!("{}{}📋 Command:{} {}", BOLD, CYAN, RESET, full_command);
}

fn execute_command(cmd: &str, args: &[&str], task_name: &str) -> bool {
    let start_time = Instant::now();

    // Display the command that will be executed
    print_command_info(cmd, args);

    let mut command = Command::new(cmd);
    for arg in args {
        command.arg(arg);
    }

    // Use inherit to display output in real time
    let status = command.status();
    let duration = start_time.elapsed();

    match status {
        Ok(status) => {
            if status.success() {
                print_success(&format!(
                    "{} completed in {:.2}s",
                    task_name,
                    duration.as_secs_f64()
                ));
                true
            } else {
                print_error(&format!(
                    "{} failed after {:.2}s",
                    task_name,
                    duration.as_secs_f64()
                ));
                false
            }
        }
        Err(e) => {
            print_error(&format!("Failed to execute {}: {}", task_name, e));
            false
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }

    match args[1].as_str() {
        "build" => build(),
        "test" => test(),
        "check" => check(),
        "format" => format(),
        "clippy" => clippy(),
        "all" => all(),
        _ => {
            print_error(&format!("Unknown command: {}", args[1]));
            println!("\n{}", YELLOW);
            print_help();
        }
    }
}

fn build() {
    print_task_start("Building All Crates", HAMMER, BLUE);
    println!(
        "{}💡 Info:{} Compiling all workspace crates in debug mode",
        BOLD, RESET
    );
    execute_command("cargo", &["build", "--workspace"], "Build");
}

fn test() {
    print_task_start("Running All Tests", TEST_TUBE, GREEN);
    println!(
        "{}💡 Info:{} Running unit tests, integration tests and doc tests",
        BOLD, RESET
    );
    execute_command("cargo", &["test", "--workspace"], "Tests");
}

fn check() {
    print_task_start("Checking All Crates", MAGNIFIER, CYAN);
    println!(
        "{}💡 Info:{} Checking code for errors without building executables",
        BOLD, RESET
    );
    execute_command("cargo", &["check", "--workspace"], "Check");
}

fn format() {
    print_task_start("Formatting Code", BRUSH, MAGENTA);
    println!(
        "{}💡 Info:{} Formatting code using rustfmt with default settings",
        BOLD, RESET
    );
    execute_command("cargo", &["fmt", "--all"], "Format");
}

fn clippy() {
    print_task_start("Running Clippy", CLIPPY, YELLOW);
    println!(
        "{}💡 Info:{} Running Clippy linter with warnings as errors",
        BOLD, RESET
    );
    execute_command(
        "cargo",
        &["clippy", "--workspace", "--", "-D", "warnings"],
        "Clippy",
    );
}

fn all() {
    print_banner();
    println!("{}{}Starting full build pipeline...{}", BOLD, CYAN, RESET);
    println!(
        "{}💡 Pipeline:{} This will run build → test → check → format → clippy",
        BOLD, RESET
    );

    let start_time = Instant::now();
    let mut success_count = 0;
    let total_tasks = 5;

    println!("\n{}{}[1/{}] Build Phase{}", BOLD, BLUE, total_tasks, RESET);
    println!(
        "{}💡 Info:{} Compiling all workspace crates in debug mode",
        BOLD, RESET
    );
    if execute_command("cargo", &["build", "--workspace"], "Build") {
        success_count += 1;
    }

    println!("\n{}{}[2/{}] Test Phase{}", BOLD, GREEN, total_tasks, RESET);
    println!(
        "{}💡 Info:{} Running unit tests, integration tests and doc tests",
        BOLD, RESET
    );
    if execute_command("cargo", &["test", "--workspace"], "Tests") {
        success_count += 1;
    }

    println!("\n{}{}[3/{}] Check Phase{}", BOLD, CYAN, total_tasks, RESET);
    println!(
        "{}💡 Info:{} Checking code for errors without building executables",
        BOLD, RESET
    );
    if execute_command("cargo", &["check", "--workspace"], "Check") {
        success_count += 1;
    }

    println!(
        "\n{}{}[4/{}] Format Phase{}",
        BOLD, MAGENTA, total_tasks, RESET
    );
    println!(
        "{}💡 Info:{} Formatting code using rustfmt with default settings",
        BOLD, RESET
    );
    if execute_command("cargo", &["fmt", "--all"], "Format") {
        success_count += 1;
    }

    println!(
        "\n{}{}[5/{}] Clippy Phase{}",
        BOLD, YELLOW, total_tasks, RESET
    );
    println!(
        "{}💡 Info:{} Running Clippy linter with warnings as errors",
        BOLD, RESET
    );
    if execute_command(
        "cargo",
        &["clippy", "--workspace", "--", "-D", "warnings"],
        "Clippy",
    ) {
        success_count += 1;
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
}
