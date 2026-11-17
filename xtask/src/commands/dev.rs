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

//! Developer-workflow shortcuts.
//!
//! These are not CI tasks — they're convenience wrappers around `cargo`
//! invocations a contributor types many times a day. The CI / engine-test
//! commands live in `commands::ci`; the asset-pipeline test fixtures in
//! `commands::assets`.

use crate::exec::*;
use crate::term::*;
use anyhow::Result;

/// Builds the editor and runtime in debug, then runs the hub in release (`cargo xtask hub-dev`).
///
/// The hub's `dev_engine()` looks for both binaries in `<workspace>/target/debug/`.
/// Two cargo invocations rather than `cargo build --workspace`: only the
/// binaries the hub launches are rebuilt.
///
/// # Errors
///
/// Fails when the build or the hub fails.
pub fn hub_dev() -> Result<()> {
    print_task_start("Hub Dev Workflow", ROCKET, CYAN);
    println!(
        "{}💡 Steps:{} (1) build editor + runtime in debug, (2) run hub in release.",
        BOLD, RESET
    );

    // The editor's Build Game stamps the runtime from `target/debug/` too.
    execute_command(
        "cargo",
        &["build", "-p", "khora-editor", "-p", "khora-runtime"],
        "Build editor + runtime (debug)",
    )?;

    execute_command(
        "cargo",
        &["run", "-p", "khora-hub", "--release"],
        "Run hub (release)",
    )?;

    Ok(())
}
