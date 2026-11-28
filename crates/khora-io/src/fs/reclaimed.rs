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

//! What was done with an entry an interrupted operation left: a folder build's, a rename's.

use std::path::PathBuf;

/// What was done with one entry an interrupted operation left beside another.
///
/// [`replace_dir_with_reporting`](super::replace_dir_with_reporting) reclaims
/// what a build left beside the folder it builds; [`reclaim_asides`](super::reclaim_asides)
/// what a rename in case only left aside.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reclaimed {
    /// A previous build an interrupted build had moved aside, put back at the
    /// folder it was moved from, which was missing.
    PutBack {
        /// Where it was left.
        from: PathBuf,
    },
    /// A folder an earlier build left, removed.
    Removed {
        /// Where it was.
        path: PathBuf,
    },
    /// A folder an earlier build left, or the previous build this one replaced,
    /// that could not be removed and is still there.
    Left {
        /// Where it is.
        path: PathBuf,
        /// Why it could not be removed.
        error: String,
    },
    /// An entry an interrupted rename had moved aside, put back under the
    /// name it had, which nothing held.
    Restored {
        /// Where it was left.
        from: PathBuf,
        /// Where it is now.
        to: PathBuf,
    },
    /// An entry an interrupted rename had moved aside, still there: never
    /// removed, as it may hold the only copy of what it holds.
    Stranded {
        /// Where it is.
        path: PathBuf,
        /// Why it could not be put back.
        error: String,
    },
}

impl std::fmt::Display for Reclaimed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reclaimed::PutBack { from } => {
                write!(
                    f,
                    "the previous build left at {} was put back",
                    from.display()
                )
            }
            Reclaimed::Removed { path } => {
                write!(
                    f,
                    "what an interrupted build left at {} was removed",
                    path.display()
                )
            }
            Reclaimed::Left { path, error } => {
                write!(
                    f,
                    "{} could not be removed and is left: {error}",
                    path.display()
                )
            }
            Reclaimed::Restored { from, to } => {
                write!(
                    f,
                    "what an interrupted rename left at {} was put back at {}",
                    from.display(),
                    to.display()
                )
            }
            Reclaimed::Stranded { path, error } => {
                write!(
                    f,
                    "what an interrupted rename left at {} is left there: {error}",
                    path.display()
                )
            }
        }
    }
}
