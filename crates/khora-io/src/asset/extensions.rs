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

//! The one table of file extensions: the asset type of a file, which its
//! index entry, its decoder and its size cap all follow.

use std::path::Path;

use khora_core::asset::AssetType;

/// Each known type and the extensions, lower case, of its files.
///
/// A texture is any image the texture decoder reads.
const EXTENSIONS: &[(AssetType, &[&str])] = &[
    (AssetType::Mesh, &["gltf", "glb", "obj", "fbx"]),
    (
        AssetType::Texture,
        &[
            "png", "jpg", "jpeg", "tga", "bmp", "hdr", "gif", "webp", "tif", "tiff", "dds", "exr",
        ],
    ),
    (AssetType::Audio, &["wav", "ogg", "mp3", "flac"]),
    (AssetType::Shader, &["wgsl", "hlsl", "glsl"]),
    (AssetType::Font, &["ttf", "otf"]),
    (AssetType::Scene, &["kscene", "scene"]),
    (AssetType::Material, &["kmat", "mat"]),
    // `kscript` is also read as a script, so a project that names its modules so still indexes.
    (
        AssetType::Script,
        &[khora_script::modules::EXTENSION, "kscript"],
    ),
    (AssetType::Prefab, &["kprefab"]),
    (AssetType::Buffer, &["bin"]),
];

/// Returns the asset type of a file ending in `ext`, in any case.
///
/// An unknown extension is a type of its own, lower-cased: every file under
/// `assets/` is indexed, decoder or not.
pub fn asset_type_for_extension(ext: &str) -> AssetType {
    let lower = ext.to_ascii_lowercase();
    EXTENSIONS
        .iter()
        .find(|(_, extensions)| extensions.contains(&lower.as_str()))
        .map(|(known, _)| known.clone())
        .unwrap_or(AssetType::Other(lower))
}

/// Returns the asset type of the file at `path`; [`AssetType::Blob`] with no extension.
pub fn asset_type_of_path(path: &Path) -> AssetType {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => asset_type_for_extension(extension),
        None => AssetType::Blob,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::FileKind;

    /// **An extension names its type in any case**; an unknown one is its own
    /// type, lower-cased.
    #[test]
    fn an_extension_names_its_type_in_any_case() {
        for (ext, ty) in [
            ("png", AssetType::Texture),
            ("PNG", AssetType::Texture),
            ("gltf", AssetType::Mesh),
            ("ogg", AssetType::Audio),
            ("kscene", AssetType::Scene),
            ("kscript", AssetType::Script),
            ("erg", AssetType::Script),
            ("kprefab", AssetType::Prefab),
            ("bin", AssetType::Buffer),
            ("xyz", AssetType::Other("xyz".to_owned())),
            ("MD", AssetType::Other("md".to_owned())),
        ] {
            assert_eq!(asset_type_for_extension(ext), ty, "{ext}");
        }
    }

    /// **A file with no extension is a blob.**
    #[test]
    fn a_file_with_no_extension_is_a_blob() {
        assert_eq!(asset_type_of_path(Path::new("notes")), AssetType::Blob);
        assert_eq!(
            asset_type_of_path(Path::new("a/b.WAV")),
            AssetType::Audio,
            "with one, its type"
        );
    }

    /// **Every image the texture decoder reads is a texture**; a `.ktx2`, which
    /// it cannot read, is not.
    #[test]
    fn every_image_the_decoder_reads_is_a_texture() {
        for ext in ["gif", "webp", "tif", "tiff", "dds", "exr"] {
            assert_eq!(asset_type_for_extension(ext), AssetType::Texture, "{ext}");
        }
        assert_eq!(
            asset_type_for_extension("ktx2"),
            AssetType::Other("ktx2".to_owned())
        );
    }

    /// **A file's kind follows its asset type**, the index's extension table
    /// the only one read.
    #[test]
    fn a_file_kind_follows_its_asset_type() {
        for (known, extensions) in EXTENSIONS {
            for ext in *extensions {
                let path = format!("a.{ext}");
                assert_eq!(
                    FileKind::of_path(Path::new(&path)),
                    FileKind::of_type(known),
                    "{path}"
                );
            }
        }
    }

    /// **No extension names two types.**
    #[test]
    fn no_extension_names_two_types() {
        let mut seen = std::collections::HashSet::new();
        for (_, extensions) in EXTENSIONS {
            for ext in *extensions {
                assert!(seen.insert(*ext), "{ext} is listed twice");
                assert_eq!(*ext, ext.to_ascii_lowercase(), "{ext} is lower case");
            }
        }
    }
}
