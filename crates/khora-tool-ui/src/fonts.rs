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

//! The brand typefaces every Khora tool installs, embedded in the binary —
//! the pair of the logo (`docs/src/design/editor.md`, "Typography"):
//!
//! - **Archivo**: every word on screen. Regular, Medium and SemiBold at normal
//!   width for text, labels and titles; Light at 112 % width for display
//!   titles; Light at 125 % width for the name KHORA (the wordmark).
//! - **IBM Plex Mono**: code, telemetry, paths, numbers — with **Noto Sans
//!   Mono**'s Greek behind it, for `ΧΩΡΑ` under the name.
//! - **Lucide**: the icon font reached through `FontFamilyHint::Icons`.
//!
//! Archivo is a variable font upstream; egui reads no variation axes, so the
//! faces here are static instances cut from the google/fonts originals.
//! All SIL Open Font License 1.1; the licenses sit next to the files in
//! `assets/fonts/`. Embedding them means a tool finds its fonts wherever its
//! binary runs, with nothing to copy next to it.

use khora_core::ui::{FontHandle, FontPack, NamedFont};

/// `(face name, bytes)`, one family at a time, in install order: the first
/// face of a family becomes its primary one, the rest its fallbacks.
type Family = &'static [(&'static str, &'static [u8])];

const PROPORTIONAL: Family = &[
    (
        "archivo-regular",
        include_bytes!("../assets/fonts/Archivo-Regular.ttf"),
    ),
    (
        "archivo-medium",
        include_bytes!("../assets/fonts/Archivo-Medium.ttf"),
    ),
    (
        "archivo-semibold",
        include_bytes!("../assets/fonts/Archivo-SemiBold.ttf"),
    ),
];

/// Plex Mono, then the Greek it lacks.
const MONOSPACE: Family = &[
    (
        "plex-mono-regular",
        include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    ),
    (
        "plex-mono-medium",
        include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf"),
    ),
    (
        "noto-mono-greek",
        include_bytes!("../assets/fonts/NotoSansMono-Greek-Light.ttf"),
    ),
];

/// Archivo, light, a little wider than the text: display titles.
const DISPLAY: Family = &[(
    "archivo-wide-light",
    include_bytes!("../assets/fonts/Archivo-Wide-Light.ttf"),
)];

/// Archivo at its widest: the name KHORA.
const WORDMARK: Family = &[(
    "archivo-expanded-light",
    include_bytes!("../assets/fonts/Archivo-Expanded-Light.ttf"),
)];

const ICONS: Family = &[("lucide", include_bytes!("../assets/fonts/Lucide.ttf"))];

fn family(faces: Family) -> Vec<NamedFont> {
    faces
        .iter()
        .map(|(name, bytes)| NamedFont {
            name: (*name).to_owned(),
            data: FontHandle::Static(bytes),
        })
        .collect()
}

/// The brand [`FontPack`]: every family, every face.
pub fn brand_fonts() -> FontPack {
    FontPack {
        proportional: family(PROPORTIONAL),
        monospace: family(MONOSPACE),
        display: family(DISPLAY),
        icons: family(ICONS),
        wordmark: family(WORDMARK),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(face name, byte length)` of every face of one family, in install
    /// order.
    fn faces(family: &[NamedFont]) -> Vec<(&str, usize)> {
        family
            .iter()
            .map(|face| (face.name.as_str(), face.data.as_bytes().len()))
            .collect()
    }

    /// Every family of the pack, named, in a fixed order.
    fn families(pack: &FontPack) -> [(&'static str, &[NamedFont]); 5] {
        [
            ("proportional", &pack.proportional),
            ("monospace", &pack.monospace),
            ("display", &pack.display),
            ("icons", &pack.icons),
            ("wordmark", &pack.wordmark),
        ]
    }

    /// Big-endian `u16` at `at`.
    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_be_bytes([bytes[at], bytes[at + 1]])
    }

    /// Big-endian `u32` at `at`.
    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    /// Whether `bytes` opens like a single TrueType / OpenType font file.
    fn is_sfnt(bytes: &[u8]) -> bool {
        bytes.len() >= 12 && matches!(&bytes[..4], [0, 1, 0, 0] | b"true" | b"OTTO")
    }

    /// The bytes of table `tag`, found through the font's table directory.
    fn table<'a>(bytes: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let count = u16_at(bytes, 4) as usize;
        (0..count)
            .map(|i| 12 + 16 * i)
            .find(|&record| &bytes[record..record + 4] == tag)
            .map(|record| {
                let offset = u32_at(bytes, record + 8) as usize;
                let length = u32_at(bytes, record + 12) as usize;
                &bytes[offset..offset + length]
            })
    }

    /// The face's `OS/2` `usWidthClass`: 5 normal, 6 semi-expanded,
    /// 7 expanded.
    fn width_class(face: &NamedFont) -> u16 {
        let os2 = table(face.data.as_bytes(), b"OS/2")
            .unwrap_or_else(|| panic!("{} has no OS/2 table", face.name));
        u16_at(os2, 6)
    }

    /// Whether the face's Unicode BMP character map (`cmap` platform 3,
    /// encoding 1, format 4) gives `ch` a glyph.
    fn maps(face: &NamedFont, ch: char) -> bool {
        let bytes = face.data.as_bytes();
        let cmap = table(bytes, b"cmap").unwrap_or_else(|| panic!("{} has no cmap", face.name));
        let subtable = (0..u16_at(cmap, 2) as usize)
            .map(|i| 4 + 8 * i)
            .find(|&record| u16_at(cmap, record) == 3 && u16_at(cmap, record + 2) == 1)
            .map(|record| &cmap[u32_at(cmap, record + 4) as usize..])
            .unwrap_or_else(|| panic!("{} has no Unicode BMP cmap", face.name));
        assert_eq!(
            u16_at(subtable, 0),
            4,
            "{}: the Unicode BMP cmap is expected in format 4",
            face.name
        );
        let cp = ch as u32;
        let seg_x2 = u16_at(subtable, 6) as usize;
        let ends = 14;
        let starts = ends + seg_x2 + 2;
        let deltas = starts + seg_x2;
        let range_offsets = deltas + seg_x2;
        for seg in 0..seg_x2 / 2 {
            if cp > u16_at(subtable, ends + 2 * seg) as u32 {
                continue;
            }
            let start = u16_at(subtable, starts + 2 * seg) as u32;
            if cp < start {
                return false;
            }
            let delta = u16_at(subtable, deltas + 2 * seg) as u32;
            let range_at = range_offsets + 2 * seg;
            let range_offset = u16_at(subtable, range_at) as usize;
            let glyph = if range_offset == 0 {
                (cp + delta) & 0xFFFF
            } else {
                let at = range_at + range_offset + 2 * (cp - start) as usize;
                match u16_at(subtable, at) as u32 {
                    0 => 0,
                    g => (g + delta) & 0xFFFF,
                }
            };
            return glyph != 0;
        }
        false
    }

    /// Every tool gets every brand family, face for face: Archivo for every
    /// word, Archivo wider for display and widest for the wordmark, IBM Plex
    /// Mono for code and numbers with a Greek-only fallback behind it, Lucide
    /// for icons.
    #[test]
    fn every_tool_loads_every_brand_family() {
        let pack = brand_fonts();

        assert_eq!(
            faces(&pack.proportional),
            [
                ("archivo-regular", 121_500),
                ("archivo-medium", 121_548),
                ("archivo-semibold", 121_280),
            ]
        );
        assert_eq!(
            faces(&pack.monospace),
            [
                ("plex-mono-regular", 135_580),
                ("plex-mono-medium", 136_704),
                ("noto-mono-greek", 12_624),
            ]
        );
        assert_eq!(faces(&pack.display), [("archivo-wide-light", 121_600)]);
        assert_eq!(faces(&pack.wordmark), [("archivo-expanded-light", 121_656)]);
        assert_eq!(faces(&pack.icons), [("lucide", 680_144)]);
    }

    /// No family is left for the backend to alias, and every face is a font
    /// file the backend can load, with the tables text layout reads.
    #[test]
    fn every_brand_family_has_faces_and_every_face_is_a_font() {
        let pack = brand_fonts();
        for (family, faces) in families(&pack) {
            assert!(!faces.is_empty(), "the {family} family has no face");
            for face in faces {
                let bytes = face.data.as_bytes();
                assert!(
                    is_sfnt(bytes),
                    "{family}/{}: not a TrueType/OpenType font",
                    face.name
                );
                for tag in [b"cmap", b"head", b"hhea", b"OS/2"] {
                    assert!(
                        table(bytes, tag).is_some(),
                        "{family}/{}: no {} table",
                        face.name,
                        String::from_utf8_lossy(tag)
                    );
                }
            }
        }
    }

    /// The Greek name under the wordmark is set in the monospace family, so
    /// one of its faces carries the capitals of the word.
    #[test]
    fn the_monospace_family_shows_greek_capitals() {
        let pack = brand_fonts();
        for (ch, name) in [
            ('\u{03A7}', "CHI"),
            ('\u{03A9}', "OMEGA"),
            ('\u{03A1}', "RHO"),
            ('\u{0391}', "ALPHA"),
        ] {
            assert!(
                pack.monospace.iter().any(|face| maps(face, ch)),
                "no monospace face maps GREEK CAPITAL LETTER {name} (U+{:04X}); faces: {:?}",
                ch as u32,
                faces(&pack.monospace)
            );
        }
    }

    /// The wordmark is the widest cut of the brand face: expanded, wider than
    /// every body and display face.
    #[test]
    fn the_wordmark_family_is_the_widest_cut() {
        let pack = brand_fonts();
        let wordmark = pack
            .wordmark
            .first()
            .expect("the wordmark family has a primary face");
        assert_eq!(
            width_class(wordmark),
            7,
            "{} is expected expanded (usWidthClass 7)",
            wordmark.name
        );
        for face in pack.proportional.iter().chain(&pack.display) {
            assert!(
                width_class(wordmark) > width_class(face),
                "{} (width class {}) is not wider than {} (width class {})",
                wordmark.name,
                width_class(wordmark),
                face.name,
                width_class(face)
            );
        }
    }

    /// Display text sets wider than body text.
    #[test]
    fn the_display_family_is_wider_than_body_text() {
        let pack = brand_fonts();
        assert!(!pack.display.is_empty(), "the display family has no face");
        for display in &pack.display {
            for body in &pack.proportional {
                assert!(
                    width_class(display) > width_class(body),
                    "{} (width class {}) is not wider than {} (width class {})",
                    display.name,
                    width_class(display),
                    body.name,
                    width_class(body)
                );
            }
        }
    }
}
