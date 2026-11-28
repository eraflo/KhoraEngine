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

//! A change to an asset file on disk, as a watcher reports it.

use khora_core::event::Supersedes;
use std::path::Path;
use std::sync::Arc;

/// The kind of change that happened to an asset on disk.
///
/// A rename is a `Removed` of the old path and a `Created` of the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetChangeKind {
    /// A new asset file appeared.
    Created,
    /// An existing asset file's bytes (or attributes) changed.
    Modified,
    /// An asset file was deleted.
    Removed,
    /// Too many changes came at once and some were dropped: everything
    /// under the root is to be read again. Its `rel_path` is empty.
    Rescan,
}

/// One filesystem change against an asset under the watched root.
#[derive(Debug, Clone)]
pub struct AssetChangeEvent {
    /// What happened.
    pub kind: AssetChangeKind,
    /// The root it happened under: `root.join(&rel_path)` is the file.
    ///
    /// Carried so the event stands on its own where several roots are watched.
    pub root: Arc<Path>,
    /// Path relative to [`root`](Self::root), forward-slash separated.
    pub rel_path: String,
}

impl AssetChangeEvent {
    /// Returns the root this change happened under: `root().join(&rel_path)` is the file.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Supersedes for AssetChangeEvent {
    // Not coalescing: the channel would scan everything queued on `notify`'s
    // thread, quadratic on a branch switch of thousands of distinct paths. One
    // save's burst arrives consecutively, so the constructor drops an immediate
    // repeat in O(1); readers de-duplicate the rest.
}
