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

//! Editor grid overlay configuration.
//!
//! The infinite ground grid is a debug / editor overlay. Like
//! [`GizmoFrame`](super::GizmoFrame), only the contract type lives in
//! `khora-data` — the GPU work is the engine-side `GridLane`'s job.
//!
//! A host application opts into the grid by enabling this config
//! (shared as an `Arc<Mutex<GridConfig>>` runtime resource). The
//! sandbox leaves it disabled — no grid; the editor enables it.

use std::sync::{Arc, Mutex};

/// Editor grid overlay state.
#[derive(Debug, Clone, Default)]
pub struct GridConfig {
    /// Whether `GridLane` should render the infinite ground grid.
    pub enabled: bool,
}

/// Shared editor-grid config — the host app enables it, `GridLane` reads it.
pub type SharedGridConfig = Arc<Mutex<GridConfig>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default() {
        assert!(!GridConfig::default().enabled);
    }
}
