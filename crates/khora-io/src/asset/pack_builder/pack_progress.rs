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

//! The steps of a pack build, for a progress bar.

/// One step of a pack build, suitable for driving a UI progress bar.
#[derive(Debug, Clone)]
pub enum PackProgress {
    /// A new asset is about to be streamed into `data.pack`.
    Started {
        /// Forward-slash relative path of the asset being processed.
        rel_path: String,
        /// 0-based index of this file in the build.
        current: usize,
        /// Total number of files in the build (constant across events).
        total: usize,
    },
    /// The build finished successfully.
    Finished {
        /// Number of assets packed.
        asset_count: usize,
        /// The size of `data.pack`'s asset region in bytes, its header aside.
        pack_bytes: u64,
    },
    /// The build failed, at any step; the `Result` of `build()` is still the one to surface.
    Failed {
        /// What failed, and why: the error of `build()`, its causes included.
        message: String,
    },
}
