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

use khora_core::asset::Asset;
use std::error::Error;

/// A trait for types that can load a specific kind of asset from a byte slice.
///
/// This represents the "Data Plane" part of asset loading. Implementors of this
/// trait are responsible for the potentially CPU-intensive work of parsing and
/// decoding raw file data into a usable, engine-ready asset type.
///
/// Each `AssetLoader` is specialized for a single asset type `A`.
pub trait AssetLoader<A: Asset> {
    /// Parses a byte slice and converts it into an instance of the asset `A`.
    ///
    /// # Parameters
    /// - `bytes`: The raw byte data read from an asset file.
    ///
    /// # Returns
    /// A `Result` containing the loaded asset on success, or a boxed dynamic
    /// error on failure. The error must be thread-safe.
    fn load(&self, bytes: &[u8]) -> Result<A, Box<dyn Error + Send + Sync>>;
}
