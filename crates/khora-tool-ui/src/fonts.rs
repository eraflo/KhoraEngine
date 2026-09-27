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

//! The brand typefaces every Khora tool installs, embedded in the binary.
//!
//! - **Geist** / **Geist Mono**: proportional and monospace UI text.
//! - **Fraunces** (72pt optical): display serif for headings and hero numerals.
//! - **Lucide**: the icon font reached through `FontFamilyHint::Icons`.
//!
//! All SIL Open Font License 1.1; the licenses sit next to the files in
//! `assets/fonts/`. Embedding them means a tool finds its fonts wherever its
//! binary runs, with nothing to copy next to it.

use khora_core::ui::{FontHandle, FontPack, NamedFont};

/// `(face name, bytes)`, one family at a time, in install order: the first
/// face of a family becomes its primary one.
type Family = &'static [(&'static str, &'static [u8])];

const PROPORTIONAL: Family = &[
    (
        "geist-regular",
        include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    ),
    (
        "geist-medium",
        include_bytes!("../assets/fonts/Geist-Medium.ttf"),
    ),
    (
        "geist-semibold",
        include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    ),
];

const MONOSPACE: Family = &[
    (
        "geist-mono-regular",
        include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    ),
    (
        "geist-mono-medium",
        include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    ),
];

/// Regular, then semibold: the shell makes the heavier face the primary
/// display face.
const DISPLAY: Family = &[
    (
        "fraunces-regular",
        include_bytes!("../assets/fonts/Fraunces-Regular.ttf"),
    ),
    (
        "fraunces-semibold",
        include_bytes!("../assets/fonts/Fraunces-SemiBold.ttf"),
    ),
];

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

    /// Every tool gets every brand family, face for face.
    #[test]
    fn every_tool_loads_every_brand_family() {
        let pack = brand_fonts();

        assert_eq!(
            faces(&pack.proportional),
            [
                ("geist-regular", 126_048),
                ("geist-medium", 127_660),
                ("geist-semibold", 127_872),
            ]
        );
        assert_eq!(
            faces(&pack.monospace),
            [
                ("geist-mono-regular", 149_284),
                ("geist-mono-medium", 150_096)
            ]
        );
        assert_eq!(
            faces(&pack.display),
            [("fraunces-regular", 95_960), ("fraunces-semibold", 105_824)]
        );
        assert_eq!(faces(&pack.icons), [("lucide", 680_144)]);
    }
}
