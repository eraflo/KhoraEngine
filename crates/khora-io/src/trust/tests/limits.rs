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

//! The limits a decoder works within, by kind and trust.

use super::MIB;
use crate::trust::{FileKind, Limits, Trust};

const GIB: u64 = 1 << 30;

/// The trusts that read with the larger limits, and those that read
/// with the strict ones.
const LARGER: [Trust; 2] = [Trust::Engine, Trust::Project];
const STRICT: [Trust; 3] = [Trust::Restricted, Trust::Imported, Trust::Save];

/// **A kind's limits follow its trust.**
///
/// As decided: a texture's file, dimension and decoded size; a glTF's file
/// and decoded vertices; a sound's file and decoded size; a font's and a
/// text's file, decoded as their bytes. A cap a kind has none of is the
/// largest value. `max_bytes` is always the file's size cap.
#[test]
fn a_kinds_limits_follow_its_trust() {
    for trust in LARGER {
        let texture = Limits::of(FileKind::Texture, trust);
        assert_eq!(texture.max_bytes, 256 * MIB, "{trust:?}");
        assert_eq!(texture.max_decoded_bytes, GIB, "{trust:?}");
        assert_eq!(texture.max_dimension, 16384, "{trust:?}");
        assert_eq!(texture.max_elements, u64::MAX, "{trust:?}");

        let gltf = Limits::of(FileKind::Gltf, trust);
        assert_eq!(gltf.max_bytes, 1024 * MIB, "{trust:?}");
        assert_eq!(gltf.max_elements, 1 << 24, "{trust:?}: today's caps");
        assert_eq!(gltf.max_dimension, u32::MAX, "{trust:?}");

        let audio = Limits::of(FileKind::Sound, trust);
        assert_eq!(audio.max_bytes, 512 * MIB, "{trust:?}");
        assert_eq!(audio.max_decoded_bytes, 2 * GIB, "{trust:?}");
        assert_eq!(audio.max_elements, u64::MAX, "{trust:?}");
        assert_eq!(audio.max_dimension, u32::MAX, "{trust:?}");

        for kind in [FileKind::Font, FileKind::Text, FileKind::Other] {
            let limits = Limits::of(kind, trust);
            assert_eq!(limits.max_bytes, 64 * MIB, "{kind:?} at {trust:?}");
            assert_eq!(
                limits.max_decoded_bytes,
                64 * MIB,
                "{kind:?} at {trust:?}: decoded as its bytes"
            );
            assert_eq!(limits.max_elements, u64::MAX, "{kind:?} at {trust:?}");
            assert_eq!(limits.max_dimension, u32::MAX, "{kind:?} at {trust:?}");
        }
    }
    for trust in STRICT {
        let texture = Limits::of(FileKind::Texture, trust);
        assert_eq!(texture.max_bytes, 64 * MIB, "{trust:?}");
        assert_eq!(texture.max_decoded_bytes, 256 * MIB, "{trust:?}");
        assert_eq!(texture.max_dimension, 8192, "{trust:?}");
        assert_eq!(texture.max_elements, u64::MAX, "{trust:?}");

        let gltf = Limits::of(FileKind::Gltf, trust);
        assert_eq!(gltf.max_bytes, 256 * MIB, "{trust:?}");
        assert_eq!(gltf.max_elements, 1 << 22, "{trust:?}: today's caps / 4");
        assert_eq!(gltf.max_dimension, u32::MAX, "{trust:?}");

        let audio = Limits::of(FileKind::Sound, trust);
        assert_eq!(audio.max_bytes, 128 * MIB, "{trust:?}");
        assert_eq!(audio.max_decoded_bytes, 512 * MIB, "{trust:?}");

        for kind in [FileKind::Font, FileKind::Text, FileKind::Other] {
            let limits = Limits::of(kind, trust);
            assert_eq!(limits.max_bytes, 16 * MIB, "{kind:?} at {trust:?}");
            assert_eq!(limits.max_decoded_bytes, 16 * MIB, "{kind:?} at {trust:?}");
        }
    }
    for kind in [
        FileKind::Texture,
        FileKind::Gltf,
        FileKind::Sound,
        FileKind::Font,
        FileKind::Text,
        FileKind::Other,
    ] {
        for trust in LARGER.into_iter().chain(STRICT) {
            assert_eq!(
                Limits::of(kind, trust).max_bytes,
                crate::trust::size_cap(kind, trust),
                "{kind:?} at {trust:?}: max_bytes is the file's cap"
            );
        }
    }
}

/// **The strictest limits are a text's strict row.**
///
/// The kind with the smallest caps, read untrusted — what a decoder stating
/// no limits of its own decodes within. No kind's strict file cap is
/// smaller.
#[test]
fn the_strictest_limits_are_a_texts_strict_row() {
    let strictest = Limits::strictest();
    assert_eq!(strictest, Limits::of(FileKind::Text, Trust::Restricted));
    assert_eq!(strictest.max_bytes, 16 * MIB);
    for kind in [
        FileKind::Texture,
        FileKind::Gltf,
        FileKind::Sound,
        FileKind::Font,
        FileKind::Other,
    ] {
        assert!(
            strictest.max_bytes <= Limits::of(kind, Trust::Restricted).max_bytes,
            "{kind:?}"
        );
    }
}

/// **Each built-in decoder has its kind's limits.**
///
/// At every trust: a texture's, a glTF's for the mesh decoders (an OBJ is
/// read as one), a sound's, a font's, a text's for the text formats.
#[test]
fn each_built_in_decoder_has_its_kinds_limits() {
    use crate::asset::decoders::audio::{SymphoniaDecoder, WavDecoder};
    use crate::asset::decoders::mesh::{
        GltfDecoder, MeshDispatcher, NoOpResourceResolver, ObjDecoder,
    };
    use crate::asset::{
        AssetDecoder, FontDecoder, MaterialDecoder, ScriptDecoder, ShaderDecoder, TextureDecoder,
    };

    let gltf = GltfDecoder::new(std::sync::Arc::new(NoOpResourceResolver));
    let dispatcher = MeshDispatcher::default();
    for trust in LARGER.into_iter().chain(STRICT) {
        let texture = Limits::of(FileKind::Texture, trust);
        let mesh = Limits::of(FileKind::Gltf, trust);
        let audio = Limits::of(FileKind::Sound, trust);
        let font = Limits::of(FileKind::Font, trust);
        let text = Limits::of(FileKind::Text, trust);

        assert_eq!(
            TextureDecoder.limits(trust),
            texture,
            "texture at {trust:?}"
        );
        assert_eq!(gltf.limits(trust), mesh, "glTF at {trust:?}");
        assert_eq!(
            dispatcher.limits(trust),
            mesh,
            "mesh dispatcher at {trust:?}"
        );
        assert_eq!(ObjDecoder.limits(trust), mesh, "OBJ at {trust:?}");
        assert_eq!(
            SymphoniaDecoder.limits(trust),
            audio,
            "Symphonia at {trust:?}"
        );
        assert_eq!(WavDecoder.limits(trust), audio, "WAV at {trust:?}");
        assert_eq!(FontDecoder.limits(trust), font, "font at {trust:?}");
        assert_eq!(ShaderDecoder.limits(trust), text, "shader at {trust:?}");
        assert_eq!(ScriptDecoder.limits(trust), text, "script at {trust:?}");
        assert_eq!(MaterialDecoder.limits(trust), text, "material at {trust:?}");
    }
}

/// A decoder that states no limits of its own.
struct Silent;

impl crate::asset::AssetDecoder<khora_core::asset::font::Font> for Silent {
    fn sniff(&self, _head: &[u8]) -> bool {
        true
    }

    fn load(
        &self,
        _bytes: &[u8],
    ) -> Result<khora_core::asset::font::Font, Box<dyn std::error::Error + Send + Sync>> {
        Err("never decodes".into())
    }
}

/// **A decoder stating no limits has the strictest.**
///
/// And the decoder registry answers a type's limits with its decoder's, or
/// `None` for a type no decoder reads.
#[test]
fn the_decoder_registry_answers_a_types_limits() {
    use crate::asset::{AssetDecoder, DecoderRegistry, TextureDecoder};
    use khora_core::asset::font::Font;
    use khora_core::renderer::api::resource::CpuTexture;

    assert_eq!(Silent.limits(Trust::Project), Limits::strictest());

    let mut registry =
        DecoderRegistry::new(std::sync::Arc::new(khora_telemetry::MetricsRegistry::new()));
    registry.register::<CpuTexture>("texture", TextureDecoder);
    registry.register::<Font>("silent", Silent);

    assert_eq!(
        registry.limits("texture", Trust::Restricted),
        Some(Limits::of(FileKind::Texture, Trust::Restricted))
    );
    assert_eq!(
        registry.limits("silent", Trust::Project),
        Some(Limits::strictest())
    );
    assert_eq!(registry.limits("nothing", Trust::Project), None);
}
