// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub services — non-UI plumbing for the launcher.
//!
//! Each module owns one external concern:
//!   - [`config`]   — local hub config (engine installs, recents)
//!   - [`download`] — async engine downloads with progress channel
//!   - [`git`]      — local + remote git init for new projects
//!   - [`project`]  — project creation, scaffolding, editor launch
//!   - [`github`]   — GitHub Releases API + OAuth device-flow auth

pub mod config;
pub mod download;
pub mod git;
pub mod github;
pub mod project;
