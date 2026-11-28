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

//! A file the engine refused to read.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{SecurityReason, Trust, TrustLayer};

/// A file the engine refused to read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecurityRefusal {
    /// The file, as it was given.
    pub path: PathBuf,
    /// How much it was trusted.
    pub trust: Trust,
    /// Where it was refused.
    pub layer: TrustLayer,
    /// Why.
    pub reason: SecurityReason,
}

impl SecurityRefusal {
    /// Returns what refused the file, in a sentence's words: `"file guard"`, `"type check"`, …
    ///
    /// A file whose bytes are not its type's was refused by the type check,
    /// whichever layer saw it.
    pub fn refused_by(&self) -> &'static str {
        if matches!(self.reason, SecurityReason::WrongType { .. }) {
            return "type check";
        }
        match self.layer {
            TrustLayer::File => "file guard",
            TrustLayer::Decoder => "decoder",
            TrustLayer::Content => "type check",
            TrustLayer::Pack => "pack",
        }
    }
}

impl std::fmt::Display for SecurityRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.path.display(), self.reason)
    }
}
