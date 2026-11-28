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

//! Why a pack build left a file out.

use crate::trust::SecurityReason;

/// Why a pack build left a file out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOutReason {
    /// The file guard refused it, the refusal reported to the build's refusals.
    ///
    /// Its path leaves the assets root, it is over its container's cap, or it
    /// is quarantined.
    Refused(SecurityReason),
    /// No file is at its path.
    Missing,
}
