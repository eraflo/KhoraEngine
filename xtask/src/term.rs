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

// ANSI color codes
pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";
pub const BLUE: &str = "\x1b[34m";
pub const YELLOW: &str = "\x1b[33m";
pub const CYAN: &str = "\x1b[36m";
pub const MAGENTA: &str = "\x1b[35m";

// Visual symbols
pub const CHECK: &str = "✓";
pub const CROSS: &str = "✗";
pub const GEAR: &str = "⚙";
pub const ROCKET: &str = "🚀";
pub const HAMMER: &str = "🔨";
pub const TEST_TUBE: &str = "🧪";
pub const MAGNIFIER: &str = "🔍";
pub const BRUSH: &str = "🎨";
pub const CLIPPY: &str = "📎";

pub const BANNER: &str = concat!(
    "\x1b[1m",
    "\x1b[36m", // BOLD, CYAN
    "╔═══════════════════════════════════════════════════════════╗\n",
    "║                    ",
    "🚀",
    " KHORA ENGINE ",
    "⚙",
    "                      ║\n", // ROCKET, GEAR
    "║                   Build Automation Tool                   ║\n",
    "╚═══════════════════════════════════════════════════════════╝",
    "\x1b[0m" // RESET
);

pub fn print_custom_help() {
    println!("{}", BANNER);
    println!(
        "\n{}{}Usage:{} cargo xtask <command>\n",
        BOLD, YELLOW, RESET
    );
    println!("{}Available commands:{}", BOLD, RESET);
    println!(
        "  {} {} {}build{}   - Build all crates in the workspace.",
        HAMMER, BLUE, BOLD, RESET
    );
    println!(
        "  {} {} {}test{}    - Run all tests in the workspace.",
        TEST_TUBE, GREEN, BOLD, RESET
    );
    println!(
        "  {} {} {}check{}   - Run `cargo check` on all crates.",
        MAGNIFIER, CYAN, BOLD, RESET
    );
    println!(
        "  {} {} {}format{}  - Format all code in the workspace.",
        BRUSH, MAGENTA, BOLD, RESET
    );
    println!(
        "  {} {} {}clippy{}  - Run clippy on all crates with warnings as errors.",
        CLIPPY, YELLOW, BOLD, RESET
    );
    println!(
        "  {} {} {}all{}     - Run all CI tasks (build, test, check, format, clippy).",
        ROCKET, RED, BOLD, RESET
    );
    println!(
        "  {} {} {}assets{}  - Commands for asset pipeline management (use `assets help` for more).",
        GEAR, CYAN, BOLD, RESET
    );
}

pub fn print_task_start(task_name: &str, emoji: &str, color: &str) {
    println!(
        "\n{}{}━━━ {} {} {}━━━{}",
        BOLD, color, emoji, task_name, emoji, RESET
    );
}

pub fn print_success(message: &str) {
    println!("{}{} {} {}{}", BOLD, GREEN, CHECK, message, RESET);
}

pub fn print_error(message: &str) {
    println!("{}{} {} {}{}", BOLD, RED, CROSS, message, RESET);
}

pub fn print_command_info(cmd: &str, args: &[&str]) {
    let full_command = format!("{} {}", cmd, args.join(" "));
    println!("{}{}📋 Command:{} {}", BOLD, CYAN, RESET, full_command);
}
