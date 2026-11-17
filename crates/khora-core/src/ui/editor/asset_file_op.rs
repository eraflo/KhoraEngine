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

//! A file operation on the project's assets, asked by the asset browser.

/// A file operation the asset browser asks for, on paths under the project's `assets/`.
///
/// Every path is forward-slash and relative to `assets/`. Each goes through
/// the project's identity registry, so references survive renames and moves.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AssetFileOp {
    /// Creates an empty folder.
    CreateFolder {
        /// The folder's path.
        path: String,
    },
    /// Creates an Ergon script, never over an existing file, then reveals it.
    CreateScript {
        /// The script's path.
        path: String,
        /// Its source.
        source: String,
    },
    /// Renames an asset, which keeps its identity.
    Rename {
        /// Its current path.
        from: String,
        /// Its new path.
        to: String,
    },
    /// Moves an asset into a folder, which keeps its identity.
    Move {
        /// The asset's path.
        path: String,
        /// The folder's path, `""` for the root.
        into: String,
    },
    /// Sends an asset to the system's recycle bin.
    Trash {
        /// The asset's path.
        path: String,
    },
    /// Copies an asset next to itself.
    Duplicate {
        /// The asset's path.
        path: String,
    },
}
