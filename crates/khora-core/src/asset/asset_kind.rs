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

//! An asset kind: a name the index, Ergon and the tools share, declared by a registration.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::OnceLock;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::AssetKindRegistration;
use crate::script::is_language_type;

/// A kind of asset the engine knows, by its registered name: `"mesh"`, `"sound"`, …
///
/// Declared with [`register_asset_kind!`](crate::register_asset_kind), which
/// gives a constant usable in patterns. A name no registration declares is
/// no kind. It is written as its plain name, and a name no registration
/// declares is refused where it is read.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct AssetKind(&'static str);

impl AssetKind {
    /// Returns the kind's name, as the index writes it: `"mesh"`, `"sound"`, …
    pub const fn name(self) -> &'static str {
        self.0
    }

    /// Returns the type Ergon spells the kind with, `None` for a kind no script holds.
    ///
    /// # Panics
    ///
    /// Panics if a registration is refused: a name or an Ergon name declared
    /// twice, a name not in lowercase ASCII, an Ergon name the language holds.
    pub fn ergon_name(self) -> Option<&'static str> {
        registry().ergon_of.get(self.0).copied().flatten()
    }

    /// Returns the registered kind named `name`, if any.
    ///
    /// # Panics
    ///
    /// Panics if a registration is refused: a name or an Ergon name declared
    /// twice, a name not in lowercase ASCII, an Ergon name the language holds.
    pub fn named(name: &str) -> Option<Self> {
        registry()
            .ergon_of
            .get_key_value(name)
            .map(|(name, _)| Self(name))
    }

    /// Returns the registered kind Ergon spells `name`, if any.
    ///
    /// # Panics
    ///
    /// Panics if a registration is refused: a name or an Ergon name declared
    /// twice, a name not in lowercase ASCII, an Ergon name the language holds.
    pub fn from_ergon_name(name: &str) -> Option<Self> {
        registry().by_ergon.get(name).copied()
    }

    /// Returns every registered kind, sorted by name.
    ///
    /// # Panics
    ///
    /// Panics if a registration is refused: a name or an Ergon name declared
    /// twice, a name not in lowercase ASCII, an Ergon name the language holds.
    pub fn all() -> Vec<Self> {
        registry().ergon_of.keys().copied().map(Self).collect()
    }

    /// Returns the kind named `name`, for the constant [`register_asset_kind!`](crate::register_asset_kind) declares.
    #[doc(hidden)]
    pub const fn __declare(name: &'static str) -> Self {
        Self(name)
    }
}

impl fmt::Display for AssetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl Serialize for AssetKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for AssetKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AssetKind, D::Error> {
        let name = String::deserialize(deserializer)?;
        AssetKind::named(&name)
            .ok_or_else(|| D::Error::custom(format!("`{name}` is no registered asset kind")))
    }
}

/// The registered kinds, read once from every [`AssetKindRegistration`].
struct KindRegistry {
    /// Each kind's Ergon name, by the kind's name, sorted.
    ergon_of: BTreeMap<&'static str, Option<&'static str>>,
    /// Each kind a script holds, by its Ergon name.
    by_ergon: HashMap<&'static str, AssetKind>,
}

/// Returns the registry of every kind submitted with [`register_asset_kind!`](crate::register_asset_kind).
fn registry() -> &'static KindRegistry {
    static REGISTRY: OnceLock<KindRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| build_registry(inventory::iter::<AssetKindRegistration>))
}

/// Builds the registry of `registrations`.
///
/// # Panics
///
/// Panics if a registration is refused: a name or an Ergon name declared
/// twice, a name not in lowercase ASCII (letters, digits, `_`), an Ergon name
/// the language holds ([`is_language_type`]).
fn build_registry<'a>(
    registrations: impl IntoIterator<Item = &'a AssetKindRegistration>,
) -> KindRegistry {
    let mut registry = KindRegistry {
        ergon_of: BTreeMap::new(),
        by_ergon: HashMap::new(),
    };
    for registration in registrations {
        let name = registration.kind.name();
        // A file reaches its kind by its extension, lowercased.
        assert!(
            is_lowercase_name(name),
            "the asset kind `{name}` is not named in lowercase ASCII"
        );
        assert!(
            registry
                .ergon_of
                .insert(name, registration.ergon_name)
                .is_none(),
            "the asset kind `{name}` is registered twice"
        );
        if let Some(ergon) = registration.ergon_name {
            assert!(
                !is_language_type(ergon),
                "the Ergon asset type `{ergon}` is a type of the language"
            );
            assert!(
                registry.by_ergon.insert(ergon, registration.kind).is_none(),
                "the Ergon asset type `{ergon}` is registered twice"
            );
        }
    }
    registry
}

/// Returns whether `name` is non-empty and only lowercase ASCII letters, digits and `_`.
fn is_lowercase_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::{build_registry, AssetKind};
    use crate::asset::{kinds, AssetKindRegistration};

    /// **Each of the engine's kinds is registered once**, listed by name in order.
    #[test]
    fn each_builtin_kind_is_registered_once() {
        let names: Vec<&str> = AssetKind::all().into_iter().map(AssetKind::name).collect();
        assert_eq!(
            names,
            [
                "font", "material", "mesh", "prefab", "scene", "script", "shader", "sound",
                "texture"
            ]
        );
        for (constant, name) in [
            (kinds::MESH, "mesh"),
            (kinds::MATERIAL, "material"),
            (kinds::TEXTURE, "texture"),
            (kinds::SOUND, "sound"),
            (kinds::FONT, "font"),
            (kinds::PREFAB, "prefab"),
            (kinds::SCENE, "scene"),
            (kinds::SHADER, "shader"),
            (kinds::SCRIPT, "script"),
        ] {
            assert_eq!(constant.name(), name);
            assert_eq!(AssetKind::named(name), Some(constant), "{name}");
        }
    }

    /// **Every kind a script holds has one Ergon name**, which reads back to it;
    /// a shader and a script have none.
    #[test]
    fn every_value_kind_has_one_ergon_name() {
        let spelled: Vec<(&str, &str)> = AssetKind::all()
            .into_iter()
            .filter_map(|kind| kind.ergon_name().map(|ergon| (kind.name(), ergon)))
            .collect();
        assert_eq!(
            spelled,
            [
                ("font", "Font"),
                ("material", "Material"),
                ("mesh", "Mesh"),
                ("prefab", "Prefab"),
                ("scene", "Scene"),
                ("sound", "Sound"),
                ("texture", "Texture"),
            ]
        );
        for kind in AssetKind::all() {
            if let Some(ergon) = kind.ergon_name() {
                assert_eq!(AssetKind::from_ergon_name(ergon), Some(kind), "{ergon}");
            }
        }
        assert_eq!(kinds::SHADER.ergon_name(), None);
        assert_eq!(kinds::SCRIPT.ergon_name(), None);
    }

    /// **A name no registration knows is refused**, never mapped onto a kind:
    /// the old `audio`, another case, the other vocabulary's spelling.
    #[test]
    fn a_name_no_registration_knows_is_refused() {
        for name in ["md", "audio", "bin", "blob", "", "Mesh", "MESH", "sounds"] {
            assert_eq!(AssetKind::named(name), None, "{name:?}");
        }
        for name in ["Shader", "Audio", "Script", "", "mesh", "Prefab?", "Entity"] {
            assert_eq!(AssetKind::from_ergon_name(name), None, "{name:?}");
        }
    }

    /// **A declared kind is named by the name it was declared with.**
    #[test]
    fn a_declared_kind_is_named_by_its_name() {
        const DECLARED: AssetKind = AssetKind::__declare("mesh");
        assert_eq!(DECLARED.name(), "mesh");
        assert_eq!(DECLARED, kinds::MESH);
    }

    /// **A kind is written as its plain name and read back by it**; a name no
    /// registration declares is refused.
    #[test]
    fn a_kind_is_written_as_its_name_and_an_unknown_name_is_refused() {
        let written = serde_json::to_string(&kinds::SOUND).expect("a kind is written");
        assert_eq!(written, "\"sound\"");
        let read: AssetKind = serde_json::from_str(&written).expect("and read back");
        assert_eq!(read, kinds::SOUND);
        assert_eq!(kinds::SOUND.to_string(), "sound");
        for unknown in ["\"audio\"", "\"md\"", "\"\""] {
            assert!(
                serde_json::from_str::<AssetKind>(unknown).is_err(),
                "{unknown} is refused"
            );
        }
    }

    /// **A kind declared under no registration has no Ergon name.**
    #[test]
    fn an_undeclared_kind_has_no_ergon_name() {
        assert_eq!(
            AssetKind::__declare("probe_unregistered").ergon_name(),
            None
        );
    }

    /// **Two registrations of one name are refused** when the registry is built.
    #[test]
    #[should_panic(expected = "the asset kind `twice` is registered twice")]
    fn a_name_registered_twice_panics() {
        let twice = AssetKind::__declare("twice");
        build_registry(&[
            AssetKindRegistration {
                kind: twice,
                ergon_name: None,
            },
            AssetKindRegistration {
                kind: twice,
                ergon_name: Some("Twice"),
            },
        ]);
    }

    /// **A kind Ergon would spell like a type of the language is refused** when the registry is built.
    ///
    /// Accepted, `Vec3` or `Color` would name the asset in every script.
    #[test]
    fn an_ergon_name_the_language_holds_is_refused() {
        for taken in [
            "int", "string", "Entity", "Duration", "Vec3", "Quat", "Color",
        ] {
            let built = std::panic::catch_unwind(|| {
                build_registry(&[AssetKindRegistration {
                    kind: AssetKind::__declare("probe"),
                    ergon_name: Some(taken),
                }])
            });
            assert!(built.is_err(), "`{taken}` is registered as an asset type");
        }
    }

    /// **A kind not named in lowercase ASCII is refused** when the registry is built.
    ///
    /// A file reaches its kind by its extension, lowercased: `LootTable` would
    /// be reached by none. Digits and `_` are kept.
    #[test]
    fn a_kind_not_named_in_lowercase_ascii_is_refused() {
        for refused in ["LootTable", "Mesh", "loot-table", "lööt", "loot table", ""] {
            let built = std::panic::catch_unwind(|| {
                build_registry(&[AssetKindRegistration {
                    kind: AssetKind::__declare(refused),
                    ergon_name: None,
                }])
            });
            assert!(built.is_err(), "`{refused}` is registered as a kind");
        }
        let built = build_registry(&[AssetKindRegistration {
            kind: AssetKind::__declare("loot_table2"),
            ergon_name: Some("LootTable"),
        }]);
        assert_eq!(built.by_ergon.len(), 1, "a lowercase name is kept");
    }

    /// **Two kinds Ergon spells alike are refused** when the registry is built.
    #[test]
    #[should_panic(expected = "the Ergon asset type `Same` is registered twice")]
    fn an_ergon_name_registered_twice_panics() {
        build_registry(&[
            AssetKindRegistration {
                kind: AssetKind::__declare("one"),
                ergon_name: Some("Same"),
            },
            AssetKindRegistration {
                kind: AssetKind::__declare("two"),
                ergon_name: Some("Same"),
            },
        ]);
    }
}
