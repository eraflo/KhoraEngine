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

//! `cargo xtask ai` — convenience wrapper around the zero-dependency Node
//! installer that generates the per-provider AI wrappers from the canonical
//! `.agent/<profile>/` documentation.
//!
//! The substance lives in `.agent/<profile>/installer/`. This command only
//! shells out to `node` so contributors can type `cargo xtask ai install all`
//! instead of the longer node invocation.

use crate::helpers::*;
use anyhow::Result;

/// Run the AI wrapper installer for the given profile, forwarding `args`
/// (e.g. `["install", "all"]`) to the Node CLI.
pub fn run(profile: &str, args: &[String]) -> Result<()> {
    print_task_start("AI Wrapper Installer", ROCKET, CYAN);

    if profile != "engine" && profile != "gamedev" {
        anyhow::bail!("unknown profile '{profile}' (expected 'engine' or 'gamedev')");
    }

    let script = format!(".agent/{profile}/installer/bin/khora-ai.mjs");
    let mut node_args: Vec<&str> = vec![script.as_str()];
    // Default to a helpful action when none is given.
    if args.is_empty() {
        node_args.push("help");
    } else {
        node_args.extend(args.iter().map(String::as_str));
    }

    execute_command("node", &node_args, &format!("khora-ai ({profile})"))
}
