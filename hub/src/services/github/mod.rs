// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! GitHub integration — OAuth device-flow auth + Releases API client.
//!
//! `releases::*` is re-exported flat so existing call sites that wrote
//! `crate::github::GithubAsset` keep working after the reorganisation.

pub mod auth;
pub mod releases;

pub use releases::*;
