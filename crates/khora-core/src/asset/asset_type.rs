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

//! The type of an indexed asset, as the index, packs and decoders name it.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::AssetKind;

/// The type of an asset in the index: what decodes it and how tools show it.
///
/// It is written as its [name](Self::as_str) (`"texture"`, `"audio"`, …), so an
/// index, a pack or a scene holds it as a plain string. A name the engine
/// does not know is kept, unchanged, as [`AssetType::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AssetType {
    /// A mesh or model file, `"mesh"`.
    Mesh,
    /// An image, `"texture"`.
    Texture,
    /// A sound, `"audio"`.
    Audio,
    /// A shader source, `"shader"`.
    Shader,
    /// A font, `"font"`.
    Font,
    /// A scene file, `"scene"`.
    Scene,
    /// An authored material, `"material"`.
    Material,
    /// An Ergon script, `"script"`.
    Script,
    /// A prefab, `"prefab"`.
    Prefab,
    /// A glTF's binary buffer, `"bin"`: no asset of its own, read by its glTF.
    Buffer,
    /// A file with no extension, `"blob"`.
    Blob,
    /// Any other type, by its name: an unknown extension, lower-cased, or a
    /// type a game registers a decoder for.
    Other(String),
}

impl AssetType {
    /// Every type the engine knows, [`AssetType::Other`] aside.
    pub const KNOWN: [AssetType; 11] = [
        AssetType::Mesh,
        AssetType::Texture,
        AssetType::Audio,
        AssetType::Shader,
        AssetType::Font,
        AssetType::Scene,
        AssetType::Material,
        AssetType::Script,
        AssetType::Prefab,
        AssetType::Buffer,
        AssetType::Blob,
    ];

    /// Returns the name the type is written as: `"texture"`, `"audio"`, …
    pub fn as_str(&self) -> &str {
        match self {
            Self::Other(name) => name,
            known => known.known_name().unwrap_or_default(),
        }
    }

    /// Returns the type written `name`; a name of no known type is [`AssetType::Other`].
    pub fn from_name(name: &str) -> AssetType {
        Self::of_known_name(name).unwrap_or_else(|| AssetType::Other(name.to_owned()))
    }

    /// Returns the known type written `name`, if any, without allocating.
    pub fn of_known_name(name: &str) -> Option<AssetType> {
        Self::KNOWN
            .into_iter()
            .find(|known| known.known_name() == Some(name))
    }

    /// Returns the kind a script names this type by, if a script can hold one.
    pub fn script_kind(&self) -> Option<AssetKind> {
        AssetKind::ALL
            .into_iter()
            .find(|kind| AssetType::from(*kind) == *self)
    }

    /// Returns the name of a known type; `None` for [`AssetType::Other`].
    fn known_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::Mesh => "mesh",
            Self::Texture => "texture",
            Self::Audio => "audio",
            Self::Shader => "shader",
            Self::Font => "font",
            Self::Scene => "scene",
            Self::Material => "material",
            Self::Script => "script",
            Self::Prefab => "prefab",
            Self::Buffer => "bin",
            Self::Blob => "blob",
            Self::Other(_) => return None,
        })
    }
}

impl From<AssetKind> for AssetType {
    fn from(kind: AssetKind) -> AssetType {
        match kind {
            AssetKind::Prefab => AssetType::Prefab,
            AssetKind::Sound => AssetType::Audio,
            AssetKind::Material => AssetType::Material,
            AssetKind::Mesh => AssetType::Mesh,
            AssetKind::Texture => AssetType::Texture,
            AssetKind::Scene => AssetType::Scene,
        }
    }
}

impl From<&str> for AssetType {
    fn from(name: &str) -> AssetType {
        AssetType::from_name(name)
    }
}

impl From<String> for AssetType {
    fn from(name: String) -> AssetType {
        Self::of_known_name(&name).unwrap_or(AssetType::Other(name))
    }
}

impl PartialEq<str> for AssetType {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for AssetType {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for AssetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for AssetType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AssetType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AssetType, D::Error> {
        String::deserialize(deserializer).map(AssetType::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Each known type is written as the index names it.**
    #[test]
    fn each_known_type_is_written_as_the_index_names_it() {
        let named: Vec<&str> = AssetType::KNOWN.iter().map(AssetType::as_str).collect();
        assert_eq!(
            named,
            [
                "mesh", "texture", "audio", "shader", "font", "scene", "material", "script",
                "prefab", "bin", "blob"
            ]
        );
    }

    /// **Each known name reads back to its type**, and through serde too.
    #[test]
    fn each_known_name_round_trips() {
        for known in AssetType::KNOWN {
            assert_eq!(AssetType::from_name(known.as_str()), known);
            assert_eq!(AssetType::from(known.as_str().to_owned()), known);
            let written = serde_json::to_string(&known).expect("an asset type is written");
            assert_eq!(written, format!("\"{}\"", known.as_str()));
            let read: AssetType = serde_json::from_str(&written).expect("and read back");
            assert_eq!(read, known);
        }
    }

    /// **An unknown name is kept as `Other` and written back unchanged**, its
    /// case included.
    #[test]
    fn an_unknown_name_is_kept_and_written_back_unchanged() {
        for name in ["md", "note", "Texture", "AUDIO", "", "ktx2 "] {
            let read = AssetType::from_name(name);
            assert_eq!(read, AssetType::Other(name.to_owned()), "{name:?}");
            assert_eq!(read.as_str(), name);
            let from_json: AssetType =
                serde_json::from_str(&serde_json::to_string(name).expect("a string is written"))
                    .expect("any string reads as a type");
            assert_eq!(from_json, read);
            assert_eq!(
                serde_json::to_string(&read).expect("an asset type is written"),
                serde_json::to_string(name).expect("a string is written"),
            );
        }
    }

    /// **A type is written as the plain string a field of type `String` was.**
    #[test]
    fn a_type_field_is_written_as_the_string_field_it_was() {
        #[derive(Serialize, Deserialize)]
        struct Before {
            asset_type_name: String,
        }
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct After {
            asset_type_name: AssetType,
        }
        for name in ["texture", "audio", "bin", "blob", "md"] {
            let before = Before {
                asset_type_name: name.to_owned(),
            };
            let written = serde_json::to_string(&before).expect("written");
            let after: After = serde_json::from_str(&written).expect("read as the new field");
            assert_eq!(after.asset_type_name.as_str(), name);
            assert_eq!(serde_json::to_string(&after).expect("written"), written);
        }
    }

    /// **A known type is matched without allocating**; no unknown name is one.
    #[test]
    fn only_a_known_name_is_a_known_type() {
        assert_eq!(AssetType::of_known_name("audio"), Some(AssetType::Audio));
        assert_eq!(AssetType::of_known_name("sound"), None);
        assert_eq!(AssetType::of_known_name("md"), None);
    }

    /// **A script's six kinds are six index types**, a sound the index's audio;
    /// no other type is a script kind.
    #[test]
    fn a_script_kind_is_its_index_type() {
        for kind in AssetKind::ALL {
            assert_eq!(AssetType::from(kind).script_kind(), Some(kind), "{kind:?}");
        }
        assert_eq!(AssetType::from(AssetKind::Sound), AssetType::Audio);
        for other in [
            AssetType::Shader,
            AssetType::Font,
            AssetType::Script,
            AssetType::Buffer,
            AssetType::Blob,
            AssetType::Other("md".to_owned()),
        ] {
            assert_eq!(other.script_kind(), None, "{other:?}");
        }
    }

    /// **A type compares equal to the string it is written as.**
    #[test]
    fn a_type_equals_its_name() {
        assert!(AssetType::Prefab == "prefab");
        assert!(AssetType::Other("md".to_owned()) == "md");
        assert!(AssetType::Audio != "sound");
        assert_eq!(AssetType::Buffer.to_string(), "bin");
    }
}
