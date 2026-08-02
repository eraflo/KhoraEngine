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

//! A `.erg` file, as the asset system holds it.
//!
//! **Source, not bytecode.** A module cannot be compiled on its own: an
//! `import` brings in another file's declarations, so a compiler needs the whole
//! reachable set and a decoder only ever sees one file's bytes. Compiling here
//! would mean compiling against whatever happened to be loaded, which is a
//! different program on a different frame.
//!
//! So the decoder does the one thing it *can* do alone — read the text and list
//! what the file asks for — and compilation happens where the whole set is
//! known.
//!
//! # Why the imports are extracted here at all
//!
//! Because the asset system already knows how to load an asset's prerequisites
//! before the asset itself, and how to notice when one of them changed. A script
//! that imported without saying so would be reloaded when its own file changed
//! and left stale when the file it depends on did — which is the harder half of
//! hot-reload, and the half that is already solved for materials and textures.

use super::Asset;

/// The text of one `.erg` file and what it imports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptModule {
    /// The file's source, verbatim.
    ///
    /// Kept rather than discarded after parsing, because a diagnostic points at
    /// a span and a span means nothing without the text it indexes — an error
    /// reported at reload time has to be able to show the line.
    pub source: String,

    /// The paths this module imports, in source order, relative to the script
    /// root.
    ///
    /// Duplicates removed: a file that imports the same module twice depends on
    /// it once, and the asset system's dependency lists are deduplicated by
    /// contract.
    pub imports: Vec<String>,
}

impl ScriptModule {
    /// A module holding `source` and the imports listed.
    pub fn new(source: impl Into<String>, imports: Vec<String>) -> Self {
        Self {
            source: source.into(),
            imports,
        }
    }

    /// Whether it depends on any other module.
    pub fn is_leaf(&self) -> bool {
        self.imports.is_empty()
    }
}

impl Asset for ScriptModule {}
