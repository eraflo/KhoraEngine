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

//! Asset decoder trait — raw bytes to typed asset.

use khora_core::asset::Asset;
use std::error::Error;

/// A trait for types that can decode a specific kind of asset from raw bytes.
///
/// Each implementation handles one asset type (e.g., `CpuTexture`, `Mesh`, `SoundData`).
/// Concrete decoders live in `khora-lanes` (they also implement `Lane` for identity).
pub trait AssetDecoder<A: Asset> {
    /// Parses a byte slice and converts it into an instance of the asset `A`.
    fn load(&self, bytes: &[u8]) -> Result<A, Box<dyn Error + Send + Sync>>;
}
