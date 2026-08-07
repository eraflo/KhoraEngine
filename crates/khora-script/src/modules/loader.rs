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

//! Where module source comes from.
//!
//! A trait rather than a filesystem call, for a reason that pays off three
//! times: the compiler stays testable without touching disk, the editor can
//! resolve imports against unsaved buffers, and the engine can serve them from
//! a packed asset archive at runtime. None of those would work if
//! `khora-script` opened files itself.

use std::collections::HashMap;

/// Supplies the source text of a module.
///
/// Implementations resolve a **normalised** path — see
/// [`normalise`](super::path::normalise) — relative to whatever they consider
/// the script root.
pub trait SourceLoader {
    /// The text of the module at `path`, or `None` if there is none.
    fn load(&self, path: &str) -> Option<String>;
}

/// A loader backed by a map, for tests and for the editor's unsaved buffers.
#[derive(Debug, Default, Clone)]
pub struct MemoryLoader {
    sources: HashMap<String, String>,
}

impl MemoryLoader {
    /// An empty loader.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module.
    pub fn with(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        self.sources.insert(path.into(), source.into());
        self
    }
}

impl SourceLoader for MemoryLoader {
    fn load(&self, path: &str) -> Option<String> {
        self.sources.get(path).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_loader_serves_what_it_was_given() {
        let loader = MemoryLoader::new()
            .with("a.erg", "fn void A() { }")
            .with("b.erg", "fn void B() { }");

        assert_eq!(loader.load("a.erg").as_deref(), Some("fn void A() { }"));
        assert_eq!(loader.load("missing.erg"), None);
    }
}
