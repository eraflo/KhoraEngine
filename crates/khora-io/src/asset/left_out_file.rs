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

//! A file a pack build left out, and why.

use super::LeftOutReason;

/// An external file a pack build left out of the pack: a container reads it, and the build could not.
///
/// The container that reads it fails to load from the pack, naming it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOutFile {
    /// The file, by its forward-slash path under the assets root.
    pub path: String,
    /// Why it was left out.
    pub reason: LeftOutReason,
}

impl std::fmt::Display for LeftOutFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.reason {
            LeftOutReason::Refused(reason) => write!(f, "{} {reason}", self.path),
            LeftOutReason::Missing => write!(f, "{} is missing", self.path),
        }
    }
}
