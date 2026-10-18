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

//! The kinds of asset a script can hold a reference to.

/// A kind of asset a script names by type: `Prefab? bullet;`.
///
/// The kind is static — it is the field's type, never part of the value,
/// which is the asset's [`AssetUUID`](super::AssetUUID) alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetKind {
    /// A prefab, `Prefab`.
    Prefab,
    /// A sound, `Sound`.
    Sound,
    /// A material, `Material`.
    Material,
    /// A mesh, `Mesh`.
    Mesh,
    /// A texture, `Texture`.
    Texture,
    /// A scene, `Scene`.
    Scene,
}

impl AssetKind {
    /// Every kind, in declaration order.
    pub const ALL: [AssetKind; 6] = [
        AssetKind::Prefab,
        AssetKind::Sound,
        AssetKind::Material,
        AssetKind::Mesh,
        AssetKind::Texture,
        AssetKind::Scene,
    ];

    /// The kind as Ergon spells its type: `"Prefab"`, `"Sound"`, …
    pub fn ergon_name(self) -> &'static str {
        match self {
            Self::Prefab => "Prefab",
            Self::Sound => "Sound",
            Self::Material => "Material",
            Self::Mesh => "Mesh",
            Self::Texture => "Texture",
            Self::Scene => "Scene",
        }
    }

    /// The kind Ergon spells `name`, if any.
    pub fn from_ergon_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.ergon_name() == name)
    }

    /// The kind as the asset index names it: `"prefab"`, `"audio"`, …
    pub fn index_name(self) -> &'static str {
        match self {
            Self::Prefab => "prefab",
            Self::Sound => "audio",
            Self::Material => "material",
            Self::Mesh => "mesh",
            Self::Texture => "texture",
            Self::Scene => "scene",
        }
    }

    /// The kind the asset index names `name`, if any.
    pub fn from_index_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.index_name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::AssetKind;

    /// **Every kind is spelled as Ergon writes its type.**
    #[test]
    fn each_kind_is_spelled_as_ergon_writes_it() {
        let spelled: Vec<&str> = AssetKind::ALL.iter().map(|k| k.ergon_name()).collect();
        assert_eq!(
            spelled,
            ["Prefab", "Sound", "Material", "Mesh", "Texture", "Scene"]
        );
    }

    /// **Every kind is named as the asset index names it** — a sound is the
    /// index's `audio`, the others their own name in lower case.
    #[test]
    fn each_kind_is_named_as_the_asset_index_names_it() {
        let named: Vec<&str> = AssetKind::ALL.iter().map(|k| k.index_name()).collect();
        assert_eq!(
            named,
            ["prefab", "audio", "material", "mesh", "texture", "scene"]
        );
    }

    /// **Both names read back to the kind they spell**, for all six, and
    /// the six are distinct.
    #[test]
    fn both_names_read_back_to_their_kind() {
        for kind in AssetKind::ALL {
            assert_eq!(
                AssetKind::from_ergon_name(kind.ergon_name()),
                Some(kind),
                "{kind:?} through its Ergon name"
            );
            assert_eq!(
                AssetKind::from_index_name(kind.index_name()),
                Some(kind),
                "{kind:?} through its index name"
            );
        }
        let distinct: std::collections::HashSet<AssetKind> = AssetKind::ALL.into_iter().collect();
        assert_eq!(distinct.len(), 6, "ALL holds six distinct kinds");
    }

    /// **A name that is no kind reads as none** — the other vocabulary's
    /// spelling included: the index's `audio` is no Ergon type, Ergon's
    /// `Sound` no index name, and case matters.
    #[test]
    fn a_name_of_no_kind_reads_as_none() {
        for name in [
            "", "audio", "prefab", "PREFAB", "Entity", "Sounds", "Prefab?",
        ] {
            assert_eq!(AssetKind::from_ergon_name(name), None, "{name:?}");
        }
        for name in ["", "Sound", "Prefab", "sound", "font", "script"] {
            assert_eq!(AssetKind::from_index_name(name), None, "{name:?}");
        }
    }
}
