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

//! **A file is decoded within its limits, and a decoder that breaks on it
//! breaks nothing else.** Through an `AssetService` reading a project's files
//! with a guarded `FileLoader`:
//! - an image announcing dimensions past its limits is refused from its
//!   header, as `OverLimit`, quarantined and reported under `Security`;
//! - a read the service trusts less (`Restricted`) has the strict limits: an
//!   image or a text the project's limits accept is refused;
//! - a decoder or a container that panics on a file becomes a refusal
//!   (`DecoderPanic`), on the synchronous load and off the frame alike, and
//!   the panic never escapes.
//!
//! Every folder here is a temporary one: no test reads or writes the user's
//! home folder.

use std::any::{Any, TypeId};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::{Asset, AssetDecoding, AssetUUID};
use khora_core::problems::{Problem, ProblemSource, Problems};
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::api::shader::CpuShaderSource;
use khora_io::asset::{
    AssetContainer, AssetDecoder, AssetService, FileLoader, IndexBuilder, SubAssetEntry,
    SubAssetListerRegistration,
};
use khora_io::trust::{FileGuard, Layer, Refusals, SecurityReason, SecurityRefusal, Trust};
use khora_telemetry::MetricsRegistry;

const MIB: u64 = 1 << 20;

// ─── The harness ────────────────────────────────────────────────────────────

/// A project's `assets/` folder served by an `AssetService` with the
/// inventory decoders, reading through a guard that reports to `refusals`,
/// which report to `problems`.
struct Served {
    _dir: tempfile::TempDir,
    problems: Arc<Problems>,
    refusals: Arc<Refusals>,
    service: AssetService,
}

/// Writes `files` under a fresh `assets/` folder and serves it.
fn serve(files: &[(&str, &[u8])]) -> Served {
    let dir = tempfile::tempdir().expect("a temporary project");
    let root = dir.path().join("assets");
    for (rel, bytes) in files {
        write(&root, rel, bytes);
    }
    serve_root(dir, root)
}

/// Serves the `assets/` folder `root` of `dir`, as it is on disk.
fn serve_root(dir: tempfile::TempDir, root: PathBuf) -> Served {
    std::fs::create_dir_all(&root).expect("the assets folder");
    let index = IndexBuilder::new(&root)
        .build_index_bytes()
        .expect("the project indexes");
    let problems = Arc::new(Problems::default());
    let refusals = Arc::new(Refusals::new(Some(Arc::clone(&problems)), None));
    let loader = FileLoader::guarded(FileGuard::new(&root, Trust::Project, Arc::clone(&refusals)));
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(MetricsRegistry::new()),
        None,
    )
    .expect("a service over the index");
    service.register_inventory_decoders();
    service.set_refusals(Arc::clone(&refusals));
    Served {
        _dir: dir,
        problems,
        refusals,
        service,
    }
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
    std::fs::write(path, bytes).expect("the file is written");
}

/// A text file of `len` bytes at `rel` under `root`: `head`, then NULs the
/// file system adds without them being written. A head longer than the
/// 512 bytes a decoder is shown keeps the file text to its sniff.
fn long_text(root: &Path, rel: &str, head: &str, len: u64) {
    write(root, rel, head.as_bytes());
    std::fs::OpenOptions::new()
        .write(true)
        .open(root.join(rel))
        .and_then(|file| file.set_len(len))
        .expect("the file is lengthened");
}

/// The quarantined refusal of the file whose path ends with `name`.
fn quarantined(refusals: &Refusals, name: &str) -> Option<SecurityRefusal> {
    refusals
        .quarantined()
        .into_iter()
        .find(|refusal| refusal.path.ends_with(name))
}

/// The `Security` problem naming the file whose path ends with `name`.
fn reported(problems: &Problems, name: &str) -> Option<Problem> {
    problems
        .of_source(&ProblemSource::Security)
        .into_iter()
        .find(|problem| {
            problem
                .module
                .as_deref()
                .is_some_and(|module| module.ends_with(name))
        })
}

/// The CRC-32 a PNG chunk ends with (ISO 3309, reflected).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Appends the PNG chunk `kind` holding `data`.
fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut checked = kind.to_vec();
    checked.extend_from_slice(data);
    png.extend_from_slice(&checked);
    png.extend_from_slice(&crc32(&checked).to_be_bytes());
}

/// A PNG of a few dozen bytes whose header announces `width` × `height`
/// 8-bit RGBA pixels, and which holds none.
fn png_announcing(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    // 8 bits per channel, RGBA, deflate, no filter method, no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    // An empty zlib stream: the image data starts, and holds no pixel.
    chunk(
        &mut png,
        b"IDAT",
        &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    chunk(&mut png, b"IEND", &[]);
    png
}

/// A real PNG of `width` × `height` pixels.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::new(width, height))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode png");
    png
}

// ─── Limits ─────────────────────────────────────────────────────────────────

/// **An image announcing huge dimensions is refused before decoding**: a PNG
/// whose header announces 100000 × 100000 pixels fails to load, and is
/// refused `OverLimit` at the decoder layer, at the project's trust —
/// quarantined by its content and reported under `Security`. An honest image
/// beside it loads.
#[test]
fn an_image_announcing_huge_dimensions_is_refused_before_decoding() {
    let mut served = serve(&[
        ("textures/huge.png", &png_announcing(100_000, 100_000)),
        ("textures/honest.png", &png(2, 2)),
    ]);

    assert!(served
        .service
        .load::<CpuTexture>(&AssetUUID::new_v5("textures/huge.png"))
        .is_err());

    let refusal = quarantined(&served.refusals, "huge.png").unwrap_or_else(|| {
        panic!(
            "the image is refused and quarantined: {:?}",
            served.refusals.quarantined()
        )
    });
    assert!(
        matches!(refusal.reason, SecurityReason::OverLimit { .. }),
        "refused over a limit, not: {:?}",
        refusal.reason
    );
    assert_eq!(refusal.layer, Layer::Decoder);
    assert_eq!(refusal.trust, Trust::Project);
    assert!(
        reported(&served.problems, "huge.png").is_some(),
        "the refusal is reported under Security: {:?}",
        served.problems.of_source(&ProblemSource::Security)
    );

    served
        .service
        .load::<CpuTexture>(&AssetUUID::new_v5("textures/honest.png"))
        .expect("an honest image loads");
}

/// **An image wider than the strict dimension is refused only in a
/// restricted read**: 16384 × 1 pixels load at the project's trust; a
/// service set to `Restricted` refuses the same file `OverLimit` at the
/// strict 8192, at the restricted trust — without decoding its pixels.
#[test]
fn an_image_wider_than_the_strict_dimension_is_refused_only_in_a_restricted_read() {
    let wide = png(16384, 1);
    let uuid = AssetUUID::new_v5("textures/wide.png");

    let mut trusted = serve(&[("textures/wide.png", &wide)]);
    let texture = trusted
        .service
        .load::<CpuTexture>(&uuid)
        .expect("16384 px load at the project's limits");
    assert_eq!(texture.size.width, 16384);

    let mut restricted = serve(&[("textures/wide.png", &wide)]);
    restricted.service.set_trust(Trust::Restricted);
    assert!(restricted.service.load::<CpuTexture>(&uuid).is_err());
    let refusal = quarantined(&restricted.refusals, "wide.png").unwrap_or_else(|| {
        panic!(
            "the image is refused at the strict limits: {:?}",
            restricted.refusals.quarantined()
        )
    });
    match &refusal.reason {
        SecurityReason::OverLimit { limit, .. } => {
            assert_eq!(*limit, 8192, "the strict dimension cap")
        }
        other => panic!("refused over a limit, not: {other:?}"),
    }
    assert_eq!(refusal.layer, Layer::Decoder);
    assert_eq!(refusal.trust, Trust::Restricted);
}

/// **A restricted read has the strict limits**: a 20 MiB shader — under a
/// trusted project's 64 MiB, over the strict 16 MiB — loads at the
/// project's trust; a service set to `Restricted` refuses it before decoding
/// it, `TooLarge` at the strict cap, and reports it.
#[test]
fn a_restricted_read_has_the_strict_limits() {
    let len = 20 * MIB;
    let uuid = AssetUUID::new_v5("shaders/long.wgsl");
    let project = |dir: tempfile::TempDir| {
        let root = dir.path().join("assets");
        // Past the head a decoder sniffs, so the NULs after are never sniffed.
        let head = "// a long shader\n".repeat(64);
        long_text(&root, "shaders/long.wgsl", &head, len);
        serve_root(dir, root)
    };

    let mut trusted = project(tempfile::tempdir().expect("a temporary project"));
    let source = trusted
        .service
        .load::<CpuShaderSource>(&uuid)
        .expect("a 20 MiB shader loads at the project's limits");
    assert_eq!(source.0.len() as u64, len);

    let mut restricted = project(tempfile::tempdir().expect("a temporary project"));
    restricted.service.set_trust(Trust::Restricted);
    assert!(restricted.service.load::<CpuShaderSource>(&uuid).is_err());

    let problem = reported(&restricted.problems, "long.wgsl").unwrap_or_else(|| {
        panic!(
            "the shader is refused and reported: {:?}",
            restricted.problems.of_source(&ProblemSource::Security)
        )
    });
    assert!(
        problem.message.contains(&len.to_string())
            && problem.message.contains(&(16 * MIB).to_string()),
        "refused as {len} bytes over the strict {} cap: {}",
        16 * MIB,
        problem.message
    );
    if let Some(refusal) = quarantined(&restricted.refusals, "long.wgsl") {
        assert_eq!(
            refusal.reason,
            SecurityReason::TooLarge {
                size: len,
                cap: 16 * MIB
            }
        );
        assert_eq!(refusal.layer, Layer::Decoder);
    }
}

// ─── Panics ─────────────────────────────────────────────────────────────────

/// What the panicking decoder and containers would have produced.
#[derive(Debug)]
struct Broken;
impl Asset for Broken {}

/// What the decoder's panic says.
const DECODER_PANIC: &str = "the decoder broke on this file";

/// A decoder that panics on every file it is given.
struct PanickingDecoder;

impl AssetDecoder<Broken> for PanickingDecoder {
    fn sniff(&self, _head: &[u8]) -> bool {
        true
    }

    fn load(&self, _bytes: &[u8]) -> Result<Broken, Box<dyn std::error::Error + Send + Sync>> {
        panic!("the decoder broke on this file")
    }
}

/// A project holding files of the type `kbroken`, decoded by the
/// [`PanickingDecoder`], and a font that decodes.
fn broken_project(files: &[&str]) -> Served {
    let mut owned: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|rel| (rel.to_string(), format!("bytes of {rel}").into_bytes()))
        .collect();
    owned.push((
        "fonts/body.ttf".to_owned(),
        b"\x00\x01\x00\x00 font".to_vec(),
    ));
    let borrowed: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(rel, bytes)| (rel.as_str(), bytes.as_slice()))
        .collect();
    let mut served = serve(&borrowed);
    served
        .service
        .register_decoder::<Broken>("kbroken", PanickingDecoder);
    served
}

/// The message of the `DecoderPanic` the file ending with `name` was
/// quarantined for; panics when it was not.
fn panic_refused(refusals: &Refusals, problems: &Problems, name: &str) -> String {
    let refusal = quarantined(refusals, name).unwrap_or_else(|| {
        panic!(
            "{name} is refused and quarantined: {:?}",
            refusals.quarantined()
        )
    });
    assert_eq!(refusal.layer, Layer::Decoder, "{name}");
    assert!(
        reported(problems, name).is_some(),
        "{name}: the refusal is reported under Security"
    );
    match refusal.reason {
        SecurityReason::DecoderPanic { message } => message,
        other => panic!("{name}: refused for its decoder's panic, not: {other:?}"),
    }
}

/// **A decoder panic becomes a refusal**: on the synchronous `load`, a
/// decoder panicking on a file does not take the caller down — the load
/// returns an error, the file is refused `DecoderPanic` with the panic's
/// message, quarantined and reported — and the service goes on serving.
#[test]
fn a_decoder_panic_becomes_a_refusal() {
    let mut served = broken_project(&["broken/a.kbroken"]);

    let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        served
            .service
            .load::<Broken>(&AssetUUID::new_v5("broken/a.kbroken"))
    }))
    .unwrap_or_else(|_| panic!("the decoder's panic escaped the synchronous load"));
    assert!(
        loaded.is_err(),
        "a file its decoder panicked on is no asset"
    );

    let message = panic_refused(&served.refusals, &served.problems, "a.kbroken");
    assert!(
        message.contains(DECODER_PANIC),
        "the refusal carries the panic's message: {message:?}"
    );

    served
        .service
        .load::<Font>(&AssetUUID::new_v5("fonts/body.ttf"))
        .expect("the service goes on serving");
}

/// **A decoder panic off the frame becomes a refusal**: the shared reader's
/// `decode` — what the `loading` family runs — and the service's
/// `AssetDecoding`, for a request filed by `request`, each return a failed
/// `Decoded` for a file its decoder panics on, never the panic; each file is
/// refused `DecoderPanic` with the panic's message.
#[test]
fn a_decoder_panic_off_the_frame_becomes_a_refusal() {
    let Served {
        _dir,
        problems,
        refusals,
        service,
    } = broken_project(&["broken/b.kbroken", "broken/c.kbroken"]);

    let reader = service.reader();
    let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reader.decode(
            &AssetUUID::new_v5("broken/b.kbroken"),
            TypeId::of::<Broken>(),
        )
    }))
    .unwrap_or_else(|_| panic!("the decoder's panic escaped the reader's decode"));
    assert!(
        decoded.value.is_err(),
        "a file its decoder panicked on is no asset"
    );

    let shared = Arc::new(Mutex::new(service));
    let c = AssetUUID::new_v5("broken/c.kbroken");
    let _ = shared.lock().expect("the service").request::<Broken>(&c);
    let filed = shared
        .lock()
        .expect("the service")
        .decode_requests()
        .drain();
    assert_eq!(filed.len(), 1, "one decode filed: {filed:?}");
    let decoding: Arc<dyn AssetDecoding> = AssetService::decoding(&shared);
    let (decoded, _lease) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decoding.decode_leased(&filed[0])
    }))
    .unwrap_or_else(|_| panic!("the decoder's panic escaped the service's decoding"));
    assert!(
        decoded.value.is_err(),
        "a file its decoder panicked on is no asset"
    );

    for name in ["b.kbroken", "c.kbroken"] {
        let message = panic_refused(&refusals, &problems, name);
        assert!(
            message.contains(DECODER_PANIC),
            "{name}: the refusal carries the panic's message: {message:?}"
        );
    }
}

/// Index type of the test container files whose `open` panics.
const PANICKING_BOX: &str = "kbrokenbox";
/// Index type of the test container files whose `extract` panics.
const PANICKING_PARTS: &str = "kbrokenparts";

/// Lists the one part every test container file holds.
fn list_one_part(_bytes: &[u8]) -> anyhow::Result<Vec<SubAssetEntry>> {
    Ok(vec![SubAssetEntry {
        label: "part".to_owned(),
        type_name: "kbrokenpart".to_owned(),
    }])
}

inventory::submit! {
    SubAssetListerRegistration {
        type_name: PANICKING_BOX,
        list: list_one_part,
    }
}

inventory::submit! {
    SubAssetListerRegistration {
        type_name: PANICKING_PARTS,
        list: list_one_part,
    }
}

/// A container that panics when it is opened, with or without limits.
struct PanickingOpen;

impl AssetContainer for PanickingOpen {
    fn open(
        &self,
        _bytes: &[u8],
        source_path: &Path,
    ) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        panic!("the container broke opening {}", source_path.display())
    }

    fn open_limited(
        &self,
        _bytes: &[u8],
        source_path: &Path,
        _limits: &khora_io::trust::Limits,
    ) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        panic!("the container broke opening {}", source_path.display())
    }

    fn extract(
        &self,
        _opened: &(dyn Any + Send + Sync),
        _container: AssetUUID,
        _label: &str,
        _type_name: &str,
    ) -> anyhow::Result<Box<dyn Any + Send>> {
        anyhow::bail!("never opened, never extracted")
    }
}

/// A container that opens, and panics when a part is extracted.
struct PanickingExtract;

impl AssetContainer for PanickingExtract {
    fn open(
        &self,
        _bytes: &[u8],
        _source_path: &Path,
    ) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        Ok(Arc::new(()))
    }

    fn open_limited(
        &self,
        _bytes: &[u8],
        _source_path: &Path,
        _limits: &khora_io::trust::Limits,
    ) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        Ok(Arc::new(()))
    }

    fn extract(
        &self,
        _opened: &(dyn Any + Send + Sync),
        _container: AssetUUID,
        label: &str,
        _type_name: &str,
    ) -> anyhow::Result<Box<dyn Any + Send>> {
        panic!("the container broke extracting {label}")
    }
}

/// **A container panic becomes a refusal**: loading a part of a container
/// whose opening panics, or whose extraction panics, returns an error, never
/// the panic; the container file is refused `DecoderPanic` with the panic's
/// message, quarantined and reported.
#[test]
fn a_container_panic_becomes_a_refusal() {
    let mut served = serve(&[
        ("models/open.kbrokenbox", b"a container that breaks opening"),
        (
            "models/parts.kbrokenparts",
            b"a container that breaks extracting",
        ),
    ]);
    served
        .service
        .register_container(PANICKING_BOX, PanickingOpen);
    served
        .service
        .register_container(PANICKING_PARTS, PanickingExtract);

    for (rel, said) in [
        ("models/open.kbrokenbox", "the container broke opening"),
        (
            "models/parts.kbrokenparts",
            "the container broke extracting",
        ),
    ] {
        let part = AssetUUID::sub_asset(AssetUUID::new_v5(rel), "part");
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            served.service.load::<Broken>(&part)
        }))
        .unwrap_or_else(|_| panic!("{rel}: the container's panic escaped the load"));
        assert!(loaded.is_err(), "{rel}: a part its container panicked on");

        let name = rel.rsplit('/').next().expect("a file name");
        let message = panic_refused(&served.refusals, &served.problems, name);
        assert!(
            message.contains(said),
            "{rel}: the refusal carries the panic's message: {message:?}"
        );
    }
}

// ─── Images inside a glTF ───────────────────────────────────────────────────

/// A `.gltf` holding one texture, its image `png` written into a `data:` URI.
fn gltf_holding_image(png: &[u8]) -> Vec<u8> {
    use base64::Engine;
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );
    serde_json::to_vec(&serde_json::json!({
        "asset": { "version": "2.0" },
        "images": [ { "uri": uri } ],
        "textures": [ { "source": 0 } ],
    }))
    .expect("the glTF serialises")
}

/// Serves `files` with the default decoders: the glTF container too.
fn serve_with_gltf(files: &[(&str, &[u8])]) -> Served {
    let mut served = serve(files);
    let root = served._dir.path().join("assets");
    served.service.register_default_decoders(&root);
    served
}

/// **A glTF's texture is decoded within the reader's limits**: the image
/// of a glTF's `texture0`, 16384 × 1 pixels, extracts at the project's
/// trust; a service set to `Restricted` refuses it `OverLimit` at the
/// strict 8192, as it refuses the same image in a file of its own.
#[test]
fn a_gltf_texture_is_decoded_within_the_reader_limits() {
    let file = gltf_holding_image(&png(16384, 1));
    let part = AssetUUID::sub_asset(AssetUUID::new_v5("models/wide.gltf"), "texture0");

    let mut trusted = serve_with_gltf(&[("models/wide.gltf", &file)]);
    let texture = trusted
        .service
        .load::<CpuTexture>(&part)
        .unwrap_or_else(|e| panic!("16384 px extract at the project's limits: {e:#}"));
    assert_eq!(texture.size.width, 16384);

    let mut restricted = serve_with_gltf(&[("models/wide.gltf", &file)]);
    restricted.service.set_trust(Trust::Restricted);
    assert!(
        restricted.service.load::<CpuTexture>(&part).is_err(),
        "16384 px are past the strict 8192"
    );
    let refusal = quarantined(&restricted.refusals, "wide.gltf").unwrap_or_else(|| {
        panic!(
            "the glTF is refused at the strict limits: {:?}",
            restricted.refusals.quarantined()
        )
    });
    match &refusal.reason {
        SecurityReason::OverLimit { limit, .. } => {
            assert_eq!(*limit, 8192, "the strict dimension cap")
        }
        other => panic!("refused over a limit, not: {other:?}"),
    }
    assert_eq!(refusal.trust, Trust::Restricted);
}

/// **An image bomb inside a glTF is refused over its limit**: a glTF whose
/// `texture0` is a PNG announcing 100000 × 100000 pixels fails to extract,
/// and the glTF is refused `OverLimit` at the decoder layer — quarantined
/// and reported under `Security`, as the same PNG in a file of its own is.
#[test]
fn an_image_bomb_inside_a_gltf_is_refused_over_its_limit() {
    let file = gltf_holding_image(&png_announcing(100_000, 100_000));
    let mut served = serve_with_gltf(&[("models/bomb.gltf", &file)]);
    let part = AssetUUID::sub_asset(AssetUUID::new_v5("models/bomb.gltf"), "texture0");

    assert!(served.service.load::<CpuTexture>(&part).is_err());

    let refusal = quarantined(&served.refusals, "bomb.gltf").unwrap_or_else(|| {
        panic!(
            "the glTF is refused and quarantined: {:?}",
            served.refusals.quarantined()
        )
    });
    assert!(
        matches!(refusal.reason, SecurityReason::OverLimit { .. }),
        "refused over a limit, not: {:?}",
        refusal.reason
    );
    assert_eq!(refusal.layer, Layer::Decoder);
    assert!(
        reported(&served.problems, "bomb.gltf").is_some(),
        "the refusal is reported under Security"
    );
}
