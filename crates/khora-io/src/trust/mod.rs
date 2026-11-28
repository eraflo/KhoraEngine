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

//! The trust boundary: how much the engine trusts a file it reads, and what
//! it refuses.

use serde::{Deserialize, Serialize};

mod file_kind;
mod guard;
mod limit_exceeded;
mod limits;
pub(crate) mod magic;
mod project;
mod project_file;
mod quarantine;
mod read_error;
mod security_reason;
mod security_refusal;
mod store;
mod text;
mod trust_layer;

pub use file_kind::{size_cap, FileKind};
pub use guard::FileGuard;
pub use limits::{LimitExceeded, Limits};
pub use project::ProjectTrustState;
pub use project_file::ProjectFile;
pub use quarantine::{QuarantineKey, Refusals};
pub use read_error::ReadError;
pub use security_reason::SecurityReason;
pub use security_refusal::SecurityRefusal;
pub use store::{ProjectTrust, TrustStore};
pub use text::looks_like_text;
pub use trust_layer::TrustLayer;

/// How much a file is trusted, from the most to the least.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Trust {
    /// The engine's own files.
    Engine,
    /// A file of a project its user trusts.
    Project,
    /// A file of a project its user has not trusted.
    Restricted,
    /// A file brought in from outside the project, or carrying the operating
    /// system's download mark.
    Imported,
    /// A game save.
    Save,
}

impl Trust {
    /// Returns whether a file read at this trust has the strict caps: anything but
    /// the engine's own files and a trusted project's.
    pub fn is_strict(self) -> bool {
        !matches!(self, Trust::Engine | Trust::Project)
    }

    /// Returns the trust in a sentence's words: `"trusted project"`, `"downloaded file"`, …
    pub fn label(self) -> &'static str {
        match self {
            Trust::Engine => "engine",
            Trust::Project => "trusted project",
            Trust::Restricted => "restricted project",
            Trust::Imported => "downloaded file",
            Trust::Save => "save",
        }
    }
}

#[cfg(test)]
mod tests;
