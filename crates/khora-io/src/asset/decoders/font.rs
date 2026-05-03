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

//! Font decoder: TTF/OTF bytes → `Font`.

use khora_core::asset::font::Font;

use crate::asset::AssetDecoder;

/// Decodes TTF/OTF font files into a `Font` asset.
///
/// For now, the decoder simply wraps the raw bytes; actual parsing happens
/// in the text renderer downstream.
#[derive(Clone, Default)]
pub struct FontDecoder;

impl AssetDecoder<Font> for FontDecoder {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<Font, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(Font {
            name: "Unknown Font".to_string(),
            data: bytes.to_vec(),
        })
    }
}
