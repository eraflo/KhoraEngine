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

//! Each built-in decoder knows the bytes its files start with.

use crate::asset::decoders::audio::{SymphoniaDecoder, WavDecoder};
use crate::asset::decoders::mesh::{GltfDecoder, MeshDispatcher, NoOpResourceResolver, ObjDecoder};
use crate::asset::{
    AssetDecoder, FontDecoder, MaterialDecoder, PrefabDecoder, ScriptDecoder, ShaderDecoder,
    TextureDecoder,
};

pub(super) const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";
const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\x00\x10JFIF\x00";
pub(super) const TEXT: &[u8] = b"this is a note, not a file of this kind\n";
const NUL: &[u8] = b"fn\0main";
const BINARY: &[u8] = b"\x00\x01\x02\x03\xFE\xFF";
const WAV: &[u8] = b"RIFF\x24\x00\x00\x00WAVEfmt ";
const OGG: &[u8] = b"OggS\x00\x02\x00\x00\x00\x00";
const FLAC: &[u8] = b"fLaC\x00\x00\x00\x22";
const MP3_TAGGED: &[u8] = b"ID3\x04\x00\x00\x00\x00\x00\x00";
const MP3_FRAME: &[u8] = b"\xFF\xFB\x90\x00\x00\x00";
const TTF: &[u8] = b"\x00\x01\x00\x00\x00\x0F\x00\x80";
const OTF: &[u8] = b"OTTO\x00\x0A\x00\x80";
const GLB: &[u8] = b"glTF\x02\x00\x00\x00";
const GLTF_JSON: &[u8] = b"  {\"asset\":{\"version\":\"2.0\"}}";
const OBJ: &[u8] = b"o cube\nv 1.0 0.0 0.0\nf 1 2 3\n";

#[test]
fn the_texture_decoder_knows_png_and_jpeg() {
    assert!(TextureDecoder.sniff(PNG));
    assert!(TextureDecoder.sniff(JPEG));
    assert!(!TextureDecoder.sniff(TEXT));
}

#[test]
fn the_text_decoders_refuse_a_nul() {
    assert!(ShaderDecoder.sniff(b"@fragment fn main() {}\n"));
    assert!(!ShaderDecoder.sniff(NUL));
    assert!(ScriptDecoder.sniff(b"behavior Guard {}\n"));
    assert!(!ScriptDecoder.sniff(NUL));
    assert!(MaterialDecoder.sniff(b"(type: \"Standard\")\n"));
    assert!(!MaterialDecoder.sniff(NUL));
    assert!(!MaterialDecoder.sniff(PNG));
}

#[test]
fn the_audio_decoders_know_their_formats() {
    let symphonia = SymphoniaDecoder;
    for (name, head) in [
        ("wav", WAV),
        ("ogg", OGG),
        ("flac", FLAC),
        ("tagged mp3", MP3_TAGGED),
        ("mp3 frame", MP3_FRAME),
    ] {
        assert!(symphonia.sniff(head), "{name}");
    }
    assert!(!symphonia.sniff(TEXT));
    assert!(WavDecoder.sniff(WAV));
    assert!(!WavDecoder.sniff(OGG));
    assert!(!WavDecoder.sniff(TEXT));
}

#[test]
fn the_font_decoder_knows_ttf_and_otf() {
    assert!(FontDecoder.sniff(TTF));
    assert!(FontDecoder.sniff(OTF));
    assert!(!FontDecoder.sniff(TEXT));
}

#[test]
fn the_mesh_decoders_know_gltf_and_obj() {
    let gltf = GltfDecoder::new(std::sync::Arc::new(NoOpResourceResolver));
    assert!(gltf.sniff(GLB));
    assert!(gltf.sniff(GLTF_JSON));
    assert!(!gltf.sniff(TEXT));
    assert!(!gltf.sniff(OBJ));

    assert!(ObjDecoder.sniff(OBJ));
    assert!(!ObjDecoder.sniff(NUL));

    let dispatcher = MeshDispatcher::default();
    assert!(dispatcher.sniff(GLB));
    assert!(dispatcher.sniff(GLTF_JSON));
    assert!(dispatcher.sniff(OBJ));
    assert!(!dispatcher.sniff(BINARY));
}

#[test]
fn the_prefab_decoder_knows_a_scene_file() {
    let mut head = khora_core::scene::HEADER_MAGIC_BYTES.to_vec();
    head.extend_from_slice(&[0; 8]);
    assert!(PrefabDecoder.sniff(&head));
    assert!(!PrefabDecoder.sniff(TEXT));
}
