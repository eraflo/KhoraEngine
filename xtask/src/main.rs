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

mod commands;
mod helpers;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(
    name = "xtask",
    author = "Khora Engine Developers",
    version,
    about = "Build and maintenance tasks for the Khora Engine workspace.",
    long_about = crate::helpers::BANNER,
    disable_help_subcommand = true
)]
struct Cli {
    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Build all crates in the workspace.
    Build,
    /// Run all tests in the workspace.
    Test,
    /// Run `cargo check` on all crates.
    Check,
    /// Format all code in the workspace.
    Format,
    /// Run clippy on all crates with warnings as errors.
    Clippy,
    /// Run all CI tasks (build, test, check, format, clippy).
    All,

    /// Commands for asset pipeline management.
    #[clap(subcommand)]
    Assets(AssetCommand),
}

#[derive(Subcommand, Debug)]
pub enum AssetCommand {
    /// Scans, builds metadata, and packs all assets into optimized archives.
    Pack,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        match command {
            Commands::Build => commands::ci::build()?,
            Commands::Test => commands::ci::test()?,
            Commands::Check => commands::ci::check()?,
            Commands::Format => commands::ci::format()?,
            Commands::Clippy => commands::ci::clippy()?,
            Commands::All => commands::ci::all()?,

            Commands::Assets(command) => match command {
                AssetCommand::Pack => commands::assets::pack()?,
            },
        }
    } else {
        helpers::print_custom_help();
    }

    Ok(())
}
