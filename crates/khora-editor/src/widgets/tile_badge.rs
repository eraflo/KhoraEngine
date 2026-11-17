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

#[cfg(test)]
mod tests {
    use super::*;

    /// **A module with nothing wrong wears no badge.**
    #[test]
    fn a_clean_module_wears_no_badge() {
        assert_eq!(tile_badge((0, 0)), None);
    }

    /// **An error beats a warning**: a module with both wears its errors.
    #[test]
    fn an_error_badge_beats_warnings() {
        assert_eq!(
            tile_badge((1, 3)),
            Some(TileBadge {
                severity: ProblemSeverity::Error,
                count: 1,
            })
        );
    }

    /// A module with only warnings wears them.
    #[test]
    fn a_module_with_only_warnings_wears_a_warning_badge() {
        assert_eq!(
            tile_badge((0, 2)),
            Some(TileBadge {
                severity: ProblemSeverity::Warning,
                count: 2,
            })
        );
    }
}
