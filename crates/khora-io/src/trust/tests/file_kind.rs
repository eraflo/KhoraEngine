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

//! The cap class of a file, by its kind, and the cap of a class at a trust.

use super::*;

/// **A file's kind comes from its extension**, as the index maps them, in
/// any case; a file of no asset, a glTF's `.bin` buffer included, is `Other`.
#[test]
fn a_file_kind_comes_from_its_extension() {
    for (path, kind) in [
        ("textures/a.png", FileKind::Texture),
        ("a.PNG", FileKind::Texture),
        ("a.jpg", FileKind::Texture),
        ("a.jpeg", FileKind::Texture),
        ("a.tga", FileKind::Texture),
        ("a.bmp", FileKind::Texture),
        ("a.hdr", FileKind::Texture),
        ("models/a.gltf", FileKind::Gltf),
        ("a.glb", FileKind::Gltf),
        ("models/a.bin", FileKind::Other),
        ("a.wav", FileKind::Sound),
        ("a.ogg", FileKind::Sound),
        ("a.mp3", FileKind::Sound),
        ("a.flac", FileKind::Sound),
        ("a.ttf", FileKind::Font),
        ("a.otf", FileKind::Font),
        ("scripts/a.erg", FileKind::Text),
        ("shaders/a.wgsl", FileKind::Text),
        ("a.kscene", FileKind::Text),
        ("a.kmat", FileKind::Text),
        ("a.xyz", FileKind::Other),
        ("no_extension", FileKind::Other),
    ] {
        assert_eq!(FileKind::of_path(Path::new(path)), kind, "{path}");
    }
}

/// **A file's kind follows the asset kind its extension names**, the index's
/// extension table the only one read: an `.fbx` is capped as a mesh, a
/// `.ktx2` the texture decoder cannot read is not a texture.
#[test]
fn a_file_kind_follows_the_type_its_extension_names() {
    for (path, kind) in [
        ("a.fbx", FileKind::Gltf),
        ("a.hlsl", FileKind::Text),
        ("a.glsl", FileKind::Text),
        ("a.scene", FileKind::Text),
        ("a.mat", FileKind::Text),
        ("a.kscript", FileKind::Text),
        ("a.kprefab", FileKind::Text),
        ("a.gif", FileKind::Texture),
        ("a.exr", FileKind::Texture),
        ("a.ktx2", FileKind::Other),
        ("a.ron", FileKind::Other),
        ("a.json", FileKind::Other),
        ("a.toml", FileKind::Other),
        ("a.txt", FileKind::Other),
    ] {
        assert_eq!(FileKind::of_path(Path::new(path)), kind, "{path}");
    }
}

/// **A file's cap follows its kind and trust.**
///
/// As decided: the larger caps for the engine's and a trusted project's files,
/// the strict ones for a restricted project, an import and a save; `Other` as
/// text.
#[test]
fn a_file_cap_follows_its_kind_and_trust() {
    let larger = [
        (FileKind::Texture, 256 * MIB),
        (FileKind::Gltf, 1024 * MIB),
        (FileKind::Sound, 512 * MIB),
        (FileKind::Font, 64 * MIB),
        (FileKind::Text, 64 * MIB),
        (FileKind::Other, 64 * MIB),
    ];
    let strict = [
        (FileKind::Texture, 64 * MIB),
        (FileKind::Gltf, 256 * MIB),
        (FileKind::Sound, 128 * MIB),
        (FileKind::Font, 16 * MIB),
        (FileKind::Text, 16 * MIB),
        (FileKind::Other, 16 * MIB),
    ];
    for (kind, cap) in larger {
        for trust in [Trust::Engine, Trust::Project] {
            assert_eq!(size_cap(kind, trust), cap, "{kind:?} at {trust:?}");
        }
    }
    for (kind, cap) in strict {
        for trust in [Trust::Restricted, Trust::Imported, Trust::Save] {
            assert_eq!(size_cap(kind, trust), cap, "{kind:?} at {trust:?}");
        }
    }
}
