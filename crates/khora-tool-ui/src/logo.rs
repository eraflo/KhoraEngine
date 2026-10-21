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

//! The Khora app icons — the window icons of the Khora tools.
//!
//! The editor wears the app icon, the silver sign on a navy tile. The hub
//! wears its negative, the navy sign on a silver tile, so the two read apart
//! in a taskbar.

/// The app icon as a 256 px PNG, embedded in the binary: the raster of
/// `docs/src/logos/khora-icon.svg`.
pub const LOGO_PNG: &[u8] = include_bytes!("../assets/khora_icon.png");

/// The hub's icon as a 256 px PNG, embedded in the binary: the app icon with
/// its colours swapped — the navy sign on the silver tile.
pub const HUB_LOGO_PNG: &[u8] = include_bytes!("../assets/khora_hub_icon.png");

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
    decode(LOGO_PNG)
}

/// Decodes [`HUB_LOGO_PNG`], with the same fallback as [`logo_icon`].
pub fn hub_logo_icon() -> LogoIcon {
    decode(HUB_LOGO_PNG)
}

fn decode(png: &[u8]) -> LogoIcon {
    match image::load_from_memory(png) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The RGBA of pixel `(x, y)` in the decoded logo.
    fn pixel(icon: &LogoIcon, x: u32, y: u32) -> [u8; 4] {
        assert!(
            x < icon.width && y < icon.height,
            "({x}, {y}) is outside the icon"
        );
        let i = ((y * icon.width + x) * 4) as usize;
        [
            icon.rgba[i],
            icon.rgba[i + 1],
            icon.rgba[i + 2],
            icon.rgba[i + 3],
        ]
    }

    fn assert_navy(p: [u8; 4], what: &str) {
        let navy = [20i32, 27, 40];
        for c in 0..3 {
            assert!(
                (p[c] as i32 - navy[c]).abs() <= 3,
                "{what}: {p:?} is not the navy tile #141b28"
            );
        }
        assert_eq!(p[3], 255, "{what}: the tile is opaque");
    }

    #[test]
    fn logo_decodes_to_a_256_pixel_square_with_a_transparent_corner() {
        let icon = logo_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        // The tile's corners are rounded: the image corner shows through.
        assert_eq!(pixel(&icon, 0, 0)[3], 0);
    }

    #[test]
    fn logo_tile_is_navy_beside_the_sign() {
        let icon = logo_icon();
        assert_navy(pixel(&icon, 20, 128), "left of the sign");
    }

    #[test]
    fn logo_sign_is_silver() {
        let icon = logo_icon();
        let p = pixel(&icon, 60, 60);
        assert!(
            p[0] > 150 && p[1] > 150 && p[2] > 150,
            "the sign's left piece is silver, got {p:?}"
        );
        assert_eq!(p[3], 255);
    }

    #[test]
    fn logo_vertical_cut_shows_the_navy_tile() {
        let icon = logo_icon();
        assert_navy(pixel(&icon, 128, 128), "the vertical cut");
    }

    fn assert_silver(p: [u8; 4], what: &str) {
        assert!(
            p[0] > 150 && p[1] > 150 && p[2] > 150,
            "{what}: {p:?} is not silver"
        );
        assert_eq!(p[3], 255, "{what}: opaque");
    }

    #[test]
    fn hub_logo_is_a_256_pixel_square_with_a_transparent_corner() {
        let icon = hub_logo_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        assert_eq!(pixel(&icon, 0, 0)[3], 0);
    }

    #[test]
    fn hub_logo_is_the_app_icon_with_its_colours_swapped() {
        let icon = hub_logo_icon();
        assert_silver(pixel(&icon, 20, 128), "the tile left of the sign");
        assert_navy(pixel(&icon, 60, 60), "the sign's left piece");
        assert_silver(pixel(&icon, 128, 128), "the vertical cut");
    }

    #[test]
    fn hub_and_editor_icons_differ() {
        assert_ne!(logo_icon(), hub_logo_icon());
    }
}
