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
//!
//! Auto-registered via [`inventory::submit!`] under the canonical `"font"`
//! slot. Single canonical implementation — no swap needed.

use khora_core::asset::font::Font;

use crate::asset::{AssetDecoder, DecoderRegistration};

/// Decodes TTF/OTF font files into a `Font` asset.
///
/// The decoder wraps the raw bytes; the text shaper and the glyph rasteriser
/// read the file when `ui_text_layout` and `glyphs_on_gpu` register it.
#[derive(Clone, Default)]
pub struct FontDecoder;

impl AssetDecoder<Font> for FontDecoder {
    fn sniff(&self, head: &[u8]) -> bool {
        [&b"\x00\x01\x00\x00"[..], b"OTTO", b"true", b"ttcf"]
            .iter()
            .any(|magic| head.starts_with(magic))
    }

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

inventory::submit! {
    DecoderRegistration {
        type_name: "font",
        register: |svc| {
            svc.register_decoder::<Font>("font", FontDecoder);
        },
    }
}
