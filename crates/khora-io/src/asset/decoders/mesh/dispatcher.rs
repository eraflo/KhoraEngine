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

//! Mesh format dispatcher: glTF or OBJ, by the file's first bytes.
//!
//! The per-format decoders stay public, so a crate can compose its own
//! dispatcher (to add a format) and register that instead.

use std::error::Error;
use std::path::Path;
use std::sync::Arc;

use khora_core::renderer::api::gpu_scene::Mesh;

use crate::asset::AssetDecoder;
use crate::trust::{FileKind, Limits, Trust};

use super::{GltfDecoder, GltfResourceResolver, NoOpResourceResolver, ObjDecoder};

/// The `"mesh"` decoder: delegates to glTF or OBJ by sniffing the bytes.
///
/// Not registered by `inventory`: its glTF resolver depends on the project or
/// pack, so [`AssetService::register_default_decoders`] registers it.
///
/// [`AssetService::register_default_decoders`]: crate::asset::AssetService::register_default_decoders
#[derive(Clone)]
pub struct MeshDispatcher {
    gltf: GltfDecoder,
    obj: ObjDecoder,
}

impl MeshDispatcher {
    /// Creates a dispatcher whose glTF decoder reads external files through `resolver`.
    pub fn new(resolver: Arc<dyn GltfResourceResolver>) -> Self {
        Self {
            gltf: GltfDecoder::new(resolver),
            obj: ObjDecoder,
        }
    }

    /// Returns true when `bytes` look like a glTF file, binary or JSON.
    pub(super) fn is_gltf(bytes: &[u8]) -> bool {
        Self::is_glb(bytes) || Self::looks_like_gltf_json(bytes)
    }

    /// Returns true when `bytes` looks like binary glTF (`glTF` magic).
    pub(super) fn is_glb(bytes: &[u8]) -> bool {
        bytes.len() >= 4 && &bytes[..4] == b"glTF"
    }

    /// Returns true when the first non-blank byte, within 64, is `{`: glTF's JSON form.
    pub(super) fn looks_like_gltf_json(bytes: &[u8]) -> bool {
        for &b in bytes.iter().take(64) {
            match b {
                b' ' | b'\t' | b'\r' | b'\n' => continue,
                b'{' => return true,
                _ => return false,
            }
        }
        false
    }
}

impl Default for MeshDispatcher {
    /// Creates a dispatcher whose glTF decoder refuses every external file.
    ///
    /// Enough for a `.glb` or an `.obj`; a `.gltf` with external files needs a
    /// [`FileSystemResolver`](super::FileSystemResolver) through [`MeshDispatcher::new`].
    fn default() -> Self {
        Self::new(Arc::new(NoOpResourceResolver))
    }
}

impl AssetDecoder<Mesh> for MeshDispatcher {
    fn sniff(&self, head: &[u8]) -> bool {
        Self::is_glb(head)
            || Self::looks_like_gltf_json(head)
            || crate::trust::looks_like_text(head)
    }

    fn limits(&self, trust: Trust) -> Limits {
        Limits::of(FileKind::Gltf, trust)
    }

    fn load(&self, bytes: &[u8]) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        self.load_at(bytes, Path::new(""))
    }

    fn load_limited(
        &self,
        bytes: &[u8],
        source_path: &Path,
        limits: &Limits,
    ) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        if Self::is_gltf(bytes) {
            self.gltf.load_limited(bytes, source_path, limits)
        } else {
            self.obj.load(bytes)
        }
    }

    /// A glTF reads its external files relative to `source_path`'s folder;
    /// an OBJ references none.
    fn load_at(
        &self,
        bytes: &[u8],
        source_path: &Path,
    ) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        if Self::is_gltf(bytes) {
            self.gltf.load_at(bytes, source_path)
        } else {
            self.obj.load(bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_glb_magic() {
        assert!(MeshDispatcher::is_glb(b"glTF\x02\x00\x00\x00"));
    }

    #[test]
    fn sniffs_gltf_json() {
        assert!(MeshDispatcher::looks_like_gltf_json(b"  {\"asset\":..."));
        assert!(MeshDispatcher::looks_like_gltf_json(b"\n\t {\"asset\""));
        assert!(!MeshDispatcher::looks_like_gltf_json(
            b"o foo\nv 1.0 0.0 0.0"
        ));
    }
}
