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

use crate::helpers::*;
use anyhow::Result;

/// `cargo xtask hub-dev` — pre-builds the engine binaries the hub needs to
/// launch (editor + runtime) in **debug** profile, then runs the hub
/// itself in **release**. Mirrors the typical contributor workflow:
///
/// - Hub in release for snappy UI.
/// - Editor + runtime in debug for fast engine-code iteration. The hub's
///   `dev_engine()` already looks for both at `<workspace>/target/debug/`,
///   so this command lines up with what the hub expects.
///
/// The two cargo invocations stay separate (rather than one
/// `cargo build --workspace`) to keep the dev cycle fast: contributors
/// rebuild only the binaries they actually need.
pub fn hub_dev() -> Result<()> {
    print_task_start("Hub Dev Workflow", ROCKET, CYAN);
    println!(
        "{}💡 Steps:{} (1) build editor + runtime in debug, (2) run hub in release.",
        BOLD, RESET
    );

    // Step 1: build editor + runtime (debug). The hub's dev_engine() expects
    // both at target/debug/, and the editor's Build Game feature stamps the
    // runtime from there.
    execute_command(
        "cargo",
        &["build", "-p", "khora-editor", "-p", "khora-runtime"],
        "Build editor + runtime (debug)",
    )?;

    // Step 2: run the hub itself in release.
    execute_command(
        "cargo",
        &["run", "-p", "khora-hub", "--release"],
        "Run hub (release)",
    )?;

    Ok(())
}
