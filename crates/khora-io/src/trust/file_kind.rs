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

//! The cap class of a file, by its asset kind, and the size cap of a class at a trust.

use std::path::Path;

use khora_core::asset::{kinds, AssetKind};

use super::Trust;

/// One kibibyte.
const KIB: u64 = 1 << 10;

/// One mebibyte.
const MIB: u64 = 1 << 20;

/// What a file is, by its asset kind, for its size cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// An image.
    Texture,
    /// A mesh file, or an external file a glTF reads, read under its glTF's cap.
    Gltf,
    /// A sound.
    Sound,
    /// A font.
    Font,
    /// A text format: script, shader, scene, material, prefab.
    Text,
    /// A project's `project.json`.
    Manifest,
    /// A project's asset identity registry, `.khora/asset-registry.ron`.
    Registry,
    /// A project's `.git/HEAD`.
    GitHead,
    /// A project's `.gitignore`.
    GitIgnore,
    /// Anything else.
    Other,
}

impl FileKind {
    /// Returns the cap class of the file at `path`, by the asset kind of its extension.
    ///
    /// A file of no asset is [`FileKind::Other`]; a container reads its
    /// external files under its own class instead ([`FileGuard::read_as`](super::FileGuard::read_as)).
    pub fn of_path(path: &Path) -> FileKind {
        crate::asset::kind_of_path(path).map_or(FileKind::Other, FileKind::of_kind)
    }

    /// Returns the cap class of an asset of `kind`; a kind with no class of its own is [`FileKind::Other`].
    pub fn of_kind(kind: AssetKind) -> FileKind {
        match kind {
            kinds::TEXTURE => FileKind::Texture,
            kinds::MESH => FileKind::Gltf,
            kinds::SOUND => FileKind::Sound,
            kinds::FONT => FileKind::Font,
            kinds::SHADER | kinds::SCENE | kinds::MATERIAL | kinds::SCRIPT | kinds::PREFAB => {
                FileKind::Text
            }
            _ => FileKind::Other,
        }
    }
}

/// Returns the size, in bytes, past which a file of `kind` read at `trust` is refused.
///
/// See the book: concepts/trust.md, "The guard".
pub fn size_cap(kind: FileKind, trust: Trust) -> u64 {
    let (larger, strict) = match kind {
        FileKind::Texture => (256 * MIB, 64 * MIB),
        FileKind::Gltf => (1024 * MIB, 256 * MIB),
        FileKind::Sound => (512 * MIB, 128 * MIB),
        FileKind::Font | FileKind::Text | FileKind::Registry | FileKind::Other => {
            (64 * MIB, 16 * MIB)
        }
        FileKind::Manifest | FileKind::GitIgnore => (MIB, 256 * KIB),
        FileKind::GitHead => (4 * KIB, 4 * KIB),
    };
    if trust.is_strict() {
        strict
    } else {
        larger
    }
}
