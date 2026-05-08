// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Marks an entity as having been instantiated from a `.kprefab` asset
/// in the project's VFS. The `source` field stores the forward-slash
/// relative path under `<project>/assets/` so the editor can offer
/// "open prefab", "revert overrides" and "spawn another" actions.
///
/// Phase 5 entry — full instance overrides + nested prefab linking is
/// scheduled for a follow-up. Today the component is purely informative:
/// the recipe stored in the .kprefab file is what actually shapes the
/// instantiated subtree.
#[derive(Debug, Clone, PartialEq, Eq, Component, Default, Serialize, Deserialize)]
pub struct Prefab {
    /// Forward-slash relative path of the source `.kprefab` asset.
    /// Example: `prefabs/crate.kprefab`.
    pub source: String,
}

impl Prefab {
    /// Creates a new `Prefab` from a relative source path.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
        }
    }
}
