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

//! The one table of file extensions: the asset kind of a file, which its
//! index entry, its decoder and its size cap all follow.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use khora_core::asset::{kinds, AssetKind};

/// Each kind a file can be and the extensions, lower case, of its files.
///
/// A texture is any image the texture decoder reads.
const EXTENSIONS: &[(AssetKind, &[&str])] = &[
    (kinds::MESH, &["gltf", "glb", "obj", "fbx"]),
    (
        kinds::TEXTURE,
        &[
            "png", "jpg", "jpeg", "tga", "bmp", "hdr", "gif", "webp", "tif", "tiff", "dds", "exr",
        ],
    ),
    (kinds::SOUND, &["wav", "ogg", "mp3", "flac"]),
    (kinds::SHADER, &["wgsl", "hlsl", "glsl"]),
    (kinds::FONT, &["ttf", "otf"]),
    (kinds::SCENE, &["kscene", "scene"]),
    (kinds::MATERIAL, &["kmat", "mat"]),
    // `kscript` is also read as a script, so a project that names its modules so still indexes.
    (
        kinds::SCRIPT,
        &[khora_script::modules::EXTENSION, "kscript"],
    ),
    (kinds::PREFAB, &["kprefab"]),
];

/// The extensions of files that are no asset, which no kind may take.
///
/// A glTF's buffer, a note, and the scratch and backup files a scan skips.
const NO_ASSET_EXTENSIONS: &[&str] = &["bin", "md", "tmp", "swp", "bak"];

/// Returns the asset kind of a file ending in `ext`, in any case.
///
/// An extension the engine's table does not list is the registered kind
/// named like it, if that kind has no row of its own: a game's kind `loot`
/// indexes its `.loot` files, while `.mesh` is no mesh. `None` for an
/// extension of no asset (`.md`, a glTF's `.bin`): such a file is not indexed.
///
/// # Panics
///
/// Panics as [`refuse_kinds_named_like_extensions`] does, which the boot runs
/// first.
pub fn kind_for_extension(ext: &str) -> Option<AssetKind> {
    extension_map()
        .get(ext.to_ascii_lowercase().as_str())
        .copied()
}

/// Returns the extension a new file of `kind` is named with, lower case: the first its row lists, else its name.
///
/// A registered kind with no row of the table is reached by its name, as
/// [`kind_for_extension`] reads it: a game's kind `loot` names its `.loot`
/// files.
pub fn extension_for_kind(kind: AssetKind) -> &'static str {
    EXTENSIONS
        .iter()
        .find(|(listed, _)| *listed == kind)
        .and_then(|(_, extensions)| extensions.first().copied())
        .unwrap_or(kind.name())
}

/// Checks that no registered kind is named like an extension of another, or of files that are no asset.
///
/// The boot runs it, so a bad kind fails there rather than at the first
/// file's lookup, on whichever thread reads it.
///
/// # Panics
///
/// Panics naming every registered kind with no row of the table that is named
/// like an extension the table lists or one of no asset (`png`, `bin`): its
/// files would never reach it, or files that are no asset would. Panics too if
/// the kinds' registrations are refused ([`AssetKind::all`]).
pub fn refuse_kinds_named_like_extensions() {
    extension_map();
}

/// Returns the asset kind of the file at `path`; `None` for a file of no asset, one with no extension included.
pub fn kind_of_path(path: &Path) -> Option<AssetKind> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(kind_for_extension)
}

/// Returns each extension's kind, built once from the kinds registered.
///
/// # Panics
///
/// As [`refuse_kinds_named_like_extensions`].
fn extension_map() -> &'static HashMap<&'static str, AssetKind> {
    static MAP: OnceLock<HashMap<&'static str, AssetKind>> = OnceLock::new();
    MAP.get_or_init(|| build_extension_map(AssetKind::all()))
}

/// Builds each extension's kind: the table's rows, then each of `kinds` with no row, by its name.
///
/// # Panics
///
/// Panics naming every kind of [`extension_clashes`].
fn build_extension_map(
    kinds: impl IntoIterator<Item = AssetKind>,
) -> HashMap<&'static str, AssetKind> {
    let kinds: Vec<AssetKind> = kinds.into_iter().collect();
    let clashes = extension_clashes(kinds.iter().copied());
    assert!(clashes.is_empty(), "{}", clashes.join("; "));
    let mut map = table();
    for kind in kinds {
        if !EXTENSIONS.iter().any(|(listed, _)| *listed == kind) {
            map.insert(kind.name(), kind);
        }
    }
    map
}

/// Returns a sentence for each of `kinds` with no row that is named like an extension the table lists or one of no asset.
fn extension_clashes(kinds: impl IntoIterator<Item = AssetKind>) -> Vec<String> {
    let listed = table();
    let mut clashes = Vec::new();
    for kind in kinds {
        if EXTENSIONS.iter().any(|(row, _)| *row == kind) {
            continue;
        }
        let name = kind.name();
        if let Some(holder) = listed.get(name) {
            clashes.push(format!(
                "the asset kind `{name}` is named like the extension of the kind `{holder}`"
            ));
        } else if NO_ASSET_EXTENSIONS.contains(&name) {
            clashes.push(format!(
                "the asset kind `{name}` is named like the extension of files that are no asset"
            ));
        }
    }
    clashes
}

/// Returns the kind of each extension the table lists.
fn table() -> HashMap<&'static str, AssetKind> {
    EXTENSIONS
        .iter()
        .flat_map(|(kind, extensions)| extensions.iter().map(move |ext| (*ext, *kind)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::FileKind;

    khora_core::register_asset_kind!(
        /// A kind a game declares, named like the extension of its files.
        const LOOT = "kloot"
    );

    /// **An extension names its kind in any case**; one of no asset names none.
    #[test]
    fn an_extension_names_its_kind_in_any_case() {
        for (ext, kind) in [
            ("png", Some(kinds::TEXTURE)),
            ("PNG", Some(kinds::TEXTURE)),
            ("gltf", Some(kinds::MESH)),
            ("ogg", Some(kinds::SOUND)),
            ("kscene", Some(kinds::SCENE)),
            ("kscript", Some(kinds::SCRIPT)),
            ("erg", Some(kinds::SCRIPT)),
            ("kprefab", Some(kinds::PREFAB)),
            ("bin", None),
            ("xyz", None),
            ("MD", None),
        ] {
            assert_eq!(kind_for_extension(ext), kind, "{ext}");
        }
    }

    /// **An extension the table does not list is the kind registered under its name**, in any case.
    #[test]
    fn an_extension_named_like_a_registered_kind_is_that_kind() {
        assert_eq!(kind_for_extension("kloot"), Some(LOOT));
        assert_eq!(kind_for_extension("KLOOT"), Some(LOOT));
        assert_eq!(kind_of_path(Path::new("tables/a.kloot")), Some(LOOT));
    }

    /// **An engine kind's name is no extension of its own**: the table lists its extensions.
    ///
    /// An Ogre `.mesh` or `.material`, or a Unity `.shader`, which no decoder reads, is no asset.
    #[test]
    fn an_engine_kinds_name_is_no_extension_of_its_own() {
        for ext in [
            "mesh", "material", "texture", "sound", "font", "prefab", "shader", "script",
        ] {
            assert_eq!(kind_for_extension(ext), None, "{ext}");
        }
    }

    /// **A new file of a kind is named with the first extension of its row**, which names
    /// that kind back; a kind with no row, with its name.
    #[test]
    fn extension_for_kind_names_a_new_file_of_that_kind() {
        assert_eq!(extension_for_kind(kinds::SCENE), "kscene");
        assert_eq!(extension_for_kind(kinds::PREFAB), "kprefab");
        assert_eq!(extension_for_kind(kinds::MESH), "gltf");
        assert_eq!(extension_for_kind(LOOT), "kloot");
        for (kind, _) in EXTENSIONS {
            assert_eq!(kind_for_extension(extension_for_kind(*kind)), Some(*kind));
        }
    }

    /// **A kind named like an extension the table lists is refused** when the map is built.
    ///
    /// Named `png`, a game's kind would never be reached; named `bin` or `md`, it
    /// would make a glTF's buffer or a note an asset.
    #[test]
    fn a_kind_named_like_a_listed_extension_is_refused() {
        for taken in ["png", "gltf", "kscript", "bin", "md"] {
            let built =
                std::panic::catch_unwind(|| build_extension_map([AssetKind::__declare(taken)]));
            assert!(built.is_err(), "`{taken}` is registered as a kind");
        }
        let map = build_extension_map([kinds::MESH, LOOT]);
        assert_eq!(map.get("kloot"), Some(&LOOT));
        assert_eq!(map.get("gltf"), Some(&kinds::MESH));
        assert_eq!(
            map.get("mesh"),
            None,
            "an engine kind is reached by its row"
        );
    }

    /// **Every kind named like an extension is found**, each named with what holds it.
    #[test]
    fn every_kind_named_like_an_extension_is_found() {
        let clashes = extension_clashes([
            AssetKind::__declare("png"),
            AssetKind::__declare("bin"),
            LOOT,
            kinds::MESH,
        ]);
        assert_eq!(clashes.len(), 2, "{clashes:?}");
        assert!(clashes[0].contains("`png`") && clashes[0].contains("texture"));
        assert!(clashes[1].contains("`bin`"));
    }

    /// **The kinds registered take no extension of another**, so the boot's check passes.
    #[test]
    fn the_registered_kinds_take_no_extension_of_another() {
        refuse_kinds_named_like_extensions();
    }

    /// **A file with no extension is no asset**; one with an asset's extension is its kind.
    #[test]
    fn a_file_with_no_extension_is_no_asset() {
        assert_eq!(kind_of_path(Path::new("notes")), None);
        assert_eq!(
            kind_of_path(Path::new("a/b.WAV")),
            Some(kinds::SOUND),
            "with one, its kind"
        );
    }

    /// **Every image the texture decoder reads is a texture**; a `.ktx2`, which
    /// it cannot read, is no asset.
    #[test]
    fn every_image_the_decoder_reads_is_a_texture() {
        for ext in ["gif", "webp", "tif", "tiff", "dds", "exr"] {
            assert_eq!(kind_for_extension(ext), Some(kinds::TEXTURE), "{ext}");
        }
        assert_eq!(kind_for_extension("ktx2"), None);
    }

    /// **A file's cap class follows its asset kind**, the index's extension table
    /// the only one read.
    #[test]
    fn a_file_kind_follows_its_asset_kind() {
        for (kind, extensions) in EXTENSIONS {
            for ext in *extensions {
                let path = format!("a.{ext}");
                assert_eq!(
                    FileKind::of_path(Path::new(&path)),
                    FileKind::of_kind(*kind),
                    "{path}"
                );
            }
        }
    }

    /// **No extension names two kinds.**
    #[test]
    fn no_extension_names_two_kinds() {
        let mut seen = std::collections::HashSet::new();
        for (_, extensions) in EXTENSIONS {
            for ext in *extensions {
                assert!(seen.insert(*ext), "{ext} is listed twice");
                assert_eq!(*ext, ext.to_ascii_lowercase(), "{ext} is lower case");
            }
        }
    }
}
