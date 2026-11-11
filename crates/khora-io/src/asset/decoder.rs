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

use crate::trust::{Limits, Trust};
use khora_core::asset::Asset;
use std::error::Error;
use std::path::Path;

/// A trait for types that can decode a specific kind of asset from raw bytes.
///
/// Each implementation handles one asset type (e.g., `CpuTexture`, `Mesh`, `SoundData`).
/// Concrete decoders live in [`crate::asset::decoders`].
pub trait AssetDecoder<A: Asset> {
    /// Parses a byte slice and converts it into an instance of the asset `A`.
    fn load(&self, bytes: &[u8]) -> Result<A, Box<dyn Error + Send + Sync>>;

    /// Decodes the asset stored at `source_path` (relative to the assets
    /// root), for formats whose files reference other files next to them.
    fn load_at(&self, bytes: &[u8], source_path: &Path) -> Result<A, Box<dyn Error + Send + Sync>> {
        let _ = source_path;
        self.load(bytes)
    }

    /// Whether `head`, the start of a file, is the start of a file this
    /// decoder reads: its magic bytes, or text (UTF-8, no NUL) for a text
    /// format. A file whose head this refuses is never decoded.
    fn sniff(&self, head: &[u8]) -> bool;

    /// The limits a file this decoder reads has at `trust`. The strictest
    /// limits by default; each built-in decoder gives its kind's.
    fn limits(&self, trust: Trust) -> Limits {
        let _ = trust;
        Limits::strictest()
    }

    /// Decodes the asset stored at `source_path` within `limits`, returning
    /// a [`LimitExceeded`](crate::trust::LimitExceeded) when one stops it.
    /// By default, [`Self::load_at`], ignoring them.
    fn load_limited(
        &self,
        bytes: &[u8],
        source_path: &Path,
        limits: &Limits,
    ) -> Result<A, Box<dyn Error + Send + Sync>> {
        let _ = limits;
        self.load_at(bytes, source_path)
    }
}
