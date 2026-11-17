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

//! What a project card shows of its folder.

use khora_sdk::trust::ProjectTrust;

/// What a project card shows of its folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardFact {
    /// What the user decided of the project; `None` when nothing was.
    pub trust: Option<ProjectTrust>,
    /// Whether the project holds a `Cargo.toml`: native Rust code.
    pub native_code: bool,
}
