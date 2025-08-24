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

use serde::Deserialize;
use std::path::PathBuf;

/// Represents the structure of the `Assets.toml` manifest file.
#[derive(Deserialize, Debug)]
pub struct AssetManifest {
    /// A list of directories to scan for source assets.
    pub source_directories: Vec<PathBuf>,
}

impl Default for AssetManifest {
    /// Provides a default configuration if `Assets.toml` is not found.
    ///
    /// The default configuration points to a single source directory:
    /// `resources/assets`.
    fn default() -> Self {
        Self {
            source_directories: vec![PathBuf::from("resources/assets")],
        }
    }
}
