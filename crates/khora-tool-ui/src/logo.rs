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

//! The Khora logo, the window icon of every Khora tool.

/// The logo as a PNG, embedded in the binary.
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/khora_small_logo.png");

/// A decoded window icon: RGBA8 pixels, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogoIcon {
    /// RGBA8 pixel buffer, row-major.
    pub rgba: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Decodes [`LOGO_PNG`]. A PNG that fails to decode logs a warning and
/// gives a transparent 1×1 icon, so a tool still opens its window.
pub fn logo_icon() -> LogoIcon {
    match image::load_from_memory(LOGO_PNG) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            LogoIcon {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        }
        Err(e) => {
            log::warn!("Failed to decode the logo PNG: {e}");
            LogoIcon {
                rgba: vec![0, 0, 0, 0],
                width: 1,
                height: 1,
            }
        }
    }
}
