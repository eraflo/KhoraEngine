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

use super::AssetDecoder;
use khora_core::asset::font::Font;
use khora_core::lane::{Lane, LaneKind};
use std::any::Any;

/// A lane dedicated to loading and decoding font files (TTF, OTF).
#[derive(Clone)]
pub struct FontLoaderLane;

impl AssetDecoder<Font> for FontLoaderLane {
    fn load(
        &self,
        bytes: &[u8],
    ) -> Result<Font, Box<dyn std::error::Error + Send + Sync + 'static>> {
        // For now, we just wrap the raw data.
        // Actual parsing (e.g. via custom parser) will happen in khora-infra's TextRenderer.
        Ok(Font {
            name: "Unknown Font".to_string(), // Metadata extraction could be added here
            data: bytes.to_vec(),
        })
    }
}

impl Lane for FontLoaderLane {
    fn strategy_name(&self) -> &'static str {
        "FontLoader"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Asset
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
