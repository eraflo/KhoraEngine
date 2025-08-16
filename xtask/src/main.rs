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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo xtask <command>");
        eprintln!("Available commands:");
        eprintln!("  build   - Build all crates");
        eprintln!("  test    - Run all tests");
        eprintln!("  check   - Run cargo check on all crates");
        eprintln!("  format  - Format all code");
        eprintln!("  clippy  - Run clippy on all crates");
        return;
    }

    match args[1].as_str() {
        "build" => build(),
        "test" => test(),
        "check" => check(),
        "format" => format(),
        "clippy" => clippy(),
        _ => eprintln!("Unknown command: {}", args[1]),
    }
}

fn build() {
    println!("Building all crates...");
    let output = Command::new("cargo")
        .arg("build")
        .arg("--workspace")
        .output()
        .expect("Failed to execute cargo build");
    
    if !output.status.success() {
        eprintln!("Build failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

fn test() {
    println!("Running all tests...");
    let output = Command::new("cargo")
        .arg("test")
        .arg("--workspace")
        .output()
        .expect("Failed to execute cargo test");
    
    if !output.status.success() {
        eprintln!("Tests failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

fn check() {
    println!("Checking all crates...");
    let output = Command::new("cargo")
        .arg("check")
        .arg("--workspace")
        .output()
        .expect("Failed to execute cargo check");
    
    if !output.status.success() {
        eprintln!("Check failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

fn format() {
    println!("Formatting all code...");
    let output = Command::new("cargo")
        .arg("fmt")
        .arg("--all")
        .output()
        .expect("Failed to execute cargo fmt");
    
    if !output.status.success() {
        eprintln!("Format failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}

fn clippy() {
    println!("Running clippy on all crates...");
    let output = Command::new("cargo")
        .arg("clippy")
        .arg("--workspace")
        .arg("--")
        .arg("-D")
        .arg("warnings")
        .output()
        .expect("Failed to execute cargo clippy");
    
    if !output.status.success() {
        eprintln!("Clippy failed: {}", String::from_utf8_lossy(&output.stderr));
    }
}
