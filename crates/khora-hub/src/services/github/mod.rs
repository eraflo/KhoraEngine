// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! GitHub integration — OAuth device-flow auth + Releases API client.
//!
//! Releases and repositories are re-exported flat: callers name
//! `github::GithubRelease`, `github::fetch_releases_async`, … directly.

pub mod auth;
pub mod releases;
pub mod repos;

pub use releases::*;
pub use repos::*;
