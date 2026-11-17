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

//! Build automation and scripting tasks for Khora Engine, run with
//! `cargo xtask <command>`.

#![expect(
    clippy::print_stdout,
    reason = "xtask is a CLI: what it prints is its output"
)]

mod commands;
mod exec;
mod term;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(
    name = "xtask",
    author = "Khora Engine Developers",
    version,
    about = "Build and maintenance tasks for the Khora Engine workspace.",
    long_about = crate::term::BANNER,
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
    /// Check the formatting of all code in the workspace.
    Format,
    /// Run clippy on all crates with warnings as errors.
    Clippy,
    /// Run all CI tasks (build, test, check, format, clippy).
    All,
    /// Run what CI checks and stop at the first failure.
    ///
    /// Format, feature unification, clippy, tests and doc tests: the check to
    /// run before a commit.
    Gate,

    /// Pre-build editor + runtime in debug, then run the hub in release.
    ///
    /// The contributor dev loop, also the `cargo hub-dev` alias from
    /// `.cargo/config.toml`.
    HubDev,

    /// Write the landing page at the root of a built book.
    ///
    /// `site/index.html`, with its stylesheet, script and frames under
    /// `site/landing/`. Run after `mdbook build`, e.g. `cargo xtask landing docs/book`.
    Landing {
        /// The built book's folder (holding `home.html`).
        site: std::path::PathBuf,
    },

    /// Run the per-file convention checks on the given Rust files.
    ///
    /// In-file order, file and function sizes, comments (plan labels,
    /// history, summary length and verb, book citations) and the crate
    /// boundaries one file shows, as `xtask/tests/layout.rs` checks them over
    /// the whole tree, without compiling. Honours the entries of
    /// `xtask/tests/layout_allow.txt`. Skips any file that is not `.rs` or lies outside `crates/`, `xtask/`
    /// and `examples/`. Prints `path:line: check: message` per finding; exits
    /// 0 when clean, 1 on a finding, 2 when a file cannot be read.
    LintFile {
        /// The files to check, relative to the current folder or absolute.
        #[clap(required = true)]
        paths: Vec<std::path::PathBuf>,
    },

    /// Commands for asset pipeline management.
    #[clap(subcommand)]
    Assets(AssetCommand),

    /// Generate the per-provider AI wrappers from `.agent/<profile>/`.
    ///
    /// Forwards to the Node installer, e.g. `cargo xtask ai install all` or
    /// `cargo xtask ai --profile gamedev install all`.
    Ai {
        /// Which documentation profile to install ("engine" or "gamedev").
        #[clap(long, default_value = "engine")]
        profile: String,
        /// Arguments forwarded to the installer (install <provider|all>, sync, …).
        #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

/// The asset pipeline's commands.
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
            Commands::Gate => commands::ci::gate()?,

            Commands::HubDev => commands::dev::hub_dev()?,
            Commands::Landing { site } => commands::landing::build(&site)?,
            Commands::LintFile { paths } => std::process::exit(commands::lint_file::run(&paths)),

            Commands::Assets(command) => match command {
                AssetCommand::Pack => commands::assets::pack()?,
            },

            Commands::Ai { profile, args } => commands::ai::run(&profile, &args)?,
        }
    } else {
        term::print_custom_help();
    }

    Ok(())
}
