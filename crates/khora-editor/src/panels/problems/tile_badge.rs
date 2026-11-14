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

//! The badge an `.erg` tile wears: the worst of its module's problems, and
//! how many.

use khora_sdk::khora_core::problems::ProblemSeverity;

/// The badge an `.erg` tile wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileBadge {
    /// The worst of what the module has.
    pub severity: ProblemSeverity,
    /// How many of that severity.
    pub count: usize,
}

/// Returns the badge of a module with `counts` errors and warnings, `None`
/// with neither; an error outranks any number of warnings.
pub(crate) fn tile_badge(counts: (usize, usize)) -> Option<TileBadge> {
    match counts {
        (0, 0) => None,
        (0, warnings) => Some(TileBadge {
            severity: ProblemSeverity::Warning,
            count: warnings,
        }),
        (errors, _) => Some(TileBadge {
            severity: ProblemSeverity::Error,
            count: errors,
        }),
    }
}
