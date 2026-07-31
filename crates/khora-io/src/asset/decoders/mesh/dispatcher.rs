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

//! Mesh format dispatcher.
//!
//! The single canonical entry-point that the editor and runtime register
//! under the `"mesh"` slot. Sniffs the bytes (gltf binary magic, gltf JSON,
//! or OBJ text) and delegates to the appropriate inner decoder.
//!
//! Lives next to the per-format decoders (`gltf.rs`, `obj.rs`) which stay
//! public — a downstream crate can compose its own dispatcher (for example
//! to add FBX) and register that instead, without forking `khora-io`.

use std::error::Error;
use std::sync::Arc;

use khora_core::renderer::api::scene::Mesh;

use crate::asset::AssetDecoder;

use super::{GltfDecoder, GltfResourceResolver, NoOpResourceResolver, ObjDecoder};

/// Default mesh dispatcher: delegates to gltf or obj based on byte sniffing.
///
/// Intentionally **not** registered via `inventory::submit!` — this slot has
/// multiple competing implementations (gltf vs obj vs eventual FBX) and the
/// choice should be visible at the consumer's call site:
///
/// ```ignore
/// svc.register_decoder::<Mesh>("mesh", MeshDispatcher::default());
/// ```
#[derive(Clone)]
pub struct MeshDispatcher {
    gltf: GltfDecoder,
    obj: ObjDecoder,
}

impl MeshDispatcher {
    /// Creates a dispatcher with the given gltf resource resolver.
    pub fn new(resolver: Arc<dyn GltfResourceResolver>) -> Self {
        Self {
            gltf: GltfDecoder::new(resolver),
            obj: ObjDecoder,
        }
    }

    /// Returns true when `bytes` looks like binary glTF (`glTF` magic).
    fn is_glb(bytes: &[u8]) -> bool {
        bytes.len() >= 4 && &bytes[..4] == b"glTF"
    }

    /// Returns true when `bytes` parses as JSON starting with a `{` after
    /// optional whitespace — the standard gltf 2.0 JSON form.
    fn looks_like_gltf_json(bytes: &[u8]) -> bool {
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
    /// Default dispatcher uses [`NoOpResourceResolver`] for gltf. Suitable
    /// for self-contained `.glb` files (the binary chunk is embedded) and
    /// `.obj` files (no external resources at all). For `.gltf` files that
    /// reference external `.bin` / texture buffers, supply a project-aware
    /// resolver via [`MeshDispatcher::new`] — `ProjectVfs::open` does this
    /// automatically with a [`FileSystemResolver`] rooted at the project's
    /// `assets/` directory.
    ///
    /// [`FileSystemResolver`]: super::FileSystemResolver
    fn default() -> Self {
        Self::new(Arc::new(NoOpResourceResolver))
    }
}

impl AssetDecoder<Mesh> for MeshDispatcher {
    fn load(&self, bytes: &[u8]) -> Result<Mesh, Box<dyn Error + Send + Sync>> {
        if Self::is_glb(bytes) || Self::looks_like_gltf_json(bytes) {
            self.gltf.load(bytes)
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
