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

//! The trust boundary, pinned: a read leaving its root, a file over its cap,
//! bytes lying about their type, a refusal reported and quarantined, and every
//! read of a project's files — assets, glTF buffers, scripts, shader overlays —
//! going through the guard.
//!
//! Every folder here is a temporary one: no test reads or writes the user's
//! home folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use tempfile::TempDir;

use super::*;

const MIB: u64 = 1 << 20;

/// A project's assets folder, `<dir>/assets`, holding
/// `textures/inside.png`, beside a folder `<dir>/elsewhere` holding
/// `secret.png`, which is under no root.
struct Layout {
    _dir: TempDir,
    root: PathBuf,
    elsewhere: PathBuf,
    outside: PathBuf,
}

fn layout() -> Layout {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    fs::create_dir_all(root.join("textures")).expect("the assets folder is made");
    fs::write(root.join("textures").join("inside.png"), b"inside bytes").expect("writes");
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("the outside folder is made");
    let outside = elsewhere.join("secret.png");
    fs::write(&outside, b"secret bytes").expect("writes");
    Layout {
        _dir: dir,
        root,
        elsewhere,
        outside,
    }
}

/// Refusals reported to `problems`, kept in memory only.
fn reported_to(problems: &Arc<Problems>) -> Arc<Refusals> {
    Arc::new(Refusals::new(Some(Arc::clone(problems)), None))
}

/// Refusals kept in memory, reported nowhere.
fn in_memory() -> Arc<Refusals> {
    Arc::new(Refusals::new(None, None))
}

/// What the security source holds.
fn security(problems: &Problems) -> Vec<Problem> {
    problems.of_source(&ProblemSource::Security)
}

/// The refusal a read returned; panics on bytes or an IO error.
fn refused(result: Result<Vec<u8>, ReadError>) -> SecurityRefusal {
    match result {
        Err(ReadError::Refused(refusal)) => refusal,
        Err(ReadError::Io(error)) => panic!("a refusal, not an IO error: {error}"),
        Ok(bytes) => panic!("a refusal, yet {} bytes were read", bytes.len()),
    }
}

/// A file of `len` bytes at `path`, made without writing them.
fn sized(path: &Path, len: u64) {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).expect("the folder is made");
    }
    fs::File::create(path)
        .and_then(|file| file.set_len(len))
        .expect("a file of the size asked for");
}

/// Makes `link` a link to the folder `target`: a symbolic link, or on Windows
/// a junction when a symbolic link needs a privilege the test lacks. False
/// when the system makes neither, and the test that needed it skips that
/// part.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success())
            && link.exists()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        false
    }
}

/// **A path escaping its root is refused**: `..` leaving it, a path through
/// a folder then `..` past the root, an absolute path, and a link inside the
/// root that resolves outside it are each refused as an `Escape`, at the file
/// layer, at the guard's trust, and reported. A file under the root reads its
/// bytes, through a link that stays inside too; a missing file is an IO
/// error, not a refusal.
#[test]
fn a_path_escaping_its_root_is_refused() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let guard = FileGuard::new(&at.root, Trust::Project, reported_to(&problems));

    assert_eq!(
        guard
            .read("textures/inside.png")
            .expect("a file under the root reads"),
        b"inside bytes"
    );

    for escaping in [
        PathBuf::from("../elsewhere/secret.png"),
        PathBuf::from("textures/../../elsewhere/secret.png"),
        at.outside.clone(),
    ] {
        let refusal = refused(guard.read(&escaping));
        assert_eq!(refusal.reason, SecurityReason::Escape, "{escaping:?}");
        assert_eq!(refusal.layer, Layer::File, "{escaping:?}");
        assert_eq!(refusal.trust, Trust::Project, "{escaping:?}");
    }

    assert!(
        matches!(guard.read("textures/missing.png"), Err(ReadError::Io(_))),
        "a missing file is an IO error"
    );
    assert!(
        !security(&problems).is_empty(),
        "every refusal is reported to the problems"
    );

    let out_link = at.root.join("linked");
    if link_dir(&at.elsewhere, &out_link) {
        let refusal = refused(guard.read("linked/secret.png"));
        assert_eq!(
            refusal.reason,
            SecurityReason::Escape,
            "a link inside the root resolving outside it"
        );
    } else {
        log::warn!("this system makes no link: the link escape is not checked");
    }
    let in_link = at.root.join("alias");
    if link_dir(&at.root.join("textures"), &in_link) {
        assert_eq!(
            guard
                .read("alias/inside.png")
                .expect("a link resolving inside the root reads"),
            b"inside bytes"
        );
    }
}

/// **A file over its cap is refused before it is read**: its size, read from
/// the metadata, is over the cap of its kind at the guard's trust — the
/// strict caps under `Restricted` and `Imported`, the larger under
/// `Project`. A file exactly at its cap reads.
#[test]
fn a_file_over_its_cap_is_refused_before_it_is_read() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    sized(&root.join("big.png"), 64 * MIB + 1);
    sized(&root.join("huge.png"), 256 * MIB + 1);
    sized(&root.join("scripts").join("long.erg"), 64 * MIB + 1);
    sized(&root.join("scripts").join("edge.erg"), 16 * MIB);

    for trust in [Trust::Restricted, Trust::Imported] {
        let guard = FileGuard::new(&root, trust, in_memory());
        let refusal = refused(guard.read("big.png"));
        assert_eq!(
            refusal.reason,
            SecurityReason::TooLarge {
                size: 64 * MIB + 1,
                cap: 64 * MIB
            },
            "{trust:?}: a texture's strict cap"
        );
        assert_eq!(refusal.layer, Layer::File);
        assert_eq!(refusal.trust, trust);

        let edge = guard.read("scripts/edge.erg").unwrap_or_else(|error| {
            panic!("{trust:?}: a text exactly at its cap reads: {error:?}")
        });
        assert_eq!(edge.len() as u64, 16 * MIB);
    }

    let project = FileGuard::new(&root, Trust::Project, in_memory());
    assert_eq!(
        refused(project.read("huge.png")).reason,
        SecurityReason::TooLarge {
            size: 256 * MIB + 1,
            cap: 256 * MIB
        },
        "a texture's project cap"
    );
    assert_eq!(
        refused(project.read("scripts/long.erg")).reason,
        SecurityReason::TooLarge {
            size: 64 * MIB + 1,
            cap: 64 * MIB
        },
        "a text's project cap"
    );
}

/// **A downloaded file is read as `Imported`**: a texture between the
/// strict cap and the project's, carrying Windows' download mark
/// (`Zone.Identifier`), is refused under `Project` trust, at the strict cap;
/// the same file without the mark reads.
#[cfg(windows)]
#[test]
fn a_downloaded_file_is_read_as_imported() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let size = 64 * MIB + 1;
    let downloaded = root.join("downloaded.png");
    sized(&downloaded, size);
    sized(&root.join("made_here.png"), size);
    let mark = format!("{}:Zone.Identifier", downloaded.display());
    if fs::write(&mark, "[ZoneTransfer]\r\nZoneId=3\r\n").is_err() {
        log::warn!("this volume keeps no download mark: the mark is not checked");
        return;
    }

    let guard = FileGuard::new(&root, Trust::Project, in_memory());

    let refusal = refused(guard.read("downloaded.png"));
    assert_eq!(
        refusal.reason,
        SecurityReason::TooLarge {
            size,
            cap: 64 * MIB
        },
        "a downloaded texture has the strict cap"
    );
    assert_eq!(refusal.trust, Trust::Imported);
    let plain = guard
        .read("made_here.png")
        .expect("a file without the mark is read at the project's trust");
    assert_eq!(plain.len() as u64, size);
}

/// **A file's kind comes from its extension**, as the index maps them, in
/// any case; a glTF's `.bin` buffer is glTF; anything else is `Other`.
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
        ("models/a.bin", FileKind::Gltf),
        ("a.wav", FileKind::Audio),
        ("a.ogg", FileKind::Audio),
        ("a.mp3", FileKind::Audio),
        ("a.flac", FileKind::Audio),
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

/// **A file's cap follows its kind and trust**, as decided: the larger caps
/// for the engine's and a trusted project's files, the strict ones for a
/// restricted project, an import and a save; `Other` as text.
#[test]
fn a_file_cap_follows_its_kind_and_trust() {
    let larger = [
        (FileKind::Texture, 256 * MIB),
        (FileKind::Gltf, 1024 * MIB),
        (FileKind::Audio, 512 * MIB),
        (FileKind::Font, 64 * MIB),
        (FileKind::Text, 64 * MIB),
        (FileKind::Other, 64 * MIB),
    ];
    let strict = [
        (FileKind::Texture, 64 * MIB),
        (FileKind::Gltf, 256 * MIB),
        (FileKind::Audio, 128 * MIB),
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

/// **Text is UTF-8 with no NUL**: a head cut in the middle of a character is
/// still text, since a head is the start of a longer file.
#[test]
fn text_is_utf8_without_a_nul() {
    assert!(looks_like_text(b"behavior Guard {}\n"));
    assert!(looks_like_text(
        "caf\u{e9} \u{2014} \u{fc}n\u{ef}code".as_bytes()
    ));
    assert!(looks_like_text(b""));
    assert!(!looks_like_text(b"fn\0main"));
    assert!(!looks_like_text(b"\xFF\xFEnot utf-8"));
    assert!(!looks_like_text(b"\x89PNG\r\n\x1a\n"));

    let cafe = "caf\u{e9}".as_bytes();
    assert!(
        looks_like_text(&cafe[..cafe.len() - 1]),
        "a head cut inside its last character"
    );
}

/// **A refused file is quarantined and reported**:
/// - a refusal with a key is quarantined, held by that key only, reported to
///   the problems under `Security` (an error located at the file, with what
///   to do), and written to the store, where new refusals over the same
///   store find it — a stamp key as well as a content key;
/// - an `Escape` is reported but never stored, whatever key it comes with;
/// - a release lets the file be read again, here and in the store, and
///   clears its problem.
#[test]
fn a_refused_file_is_quarantined_and_reported() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let problems = Arc::new(Problems::default());
    let refusals = Refusals::new(Some(Arc::clone(&problems)), Some(store.clone()));

    let liar = dir.path().join("assets").join("liar.png");
    let wrong_type = SecurityRefusal {
        path: liar.clone(),
        trust: Trust::Project,
        layer: Layer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    };
    let content = QuarantineKey::Content([7; 32]);
    refusals.refuse(wrong_type.clone(), Some(content));

    assert_eq!(refusals.quarantined(), vec![wrong_type.clone()]);
    assert!(refusals.is_quarantined(&liar, &content));
    assert!(
        !refusals.is_quarantined(&liar, &QuarantineKey::Content([8; 32])),
        "another content of the file is not quarantined"
    );
    let reported = security(&problems);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].severity, ProblemSeverity::Error);
    assert_eq!(reported[0].module, Some(liar.display().to_string()));
    assert!(!reported[0].message.is_empty());
    assert!(
        reported[0]
            .note
            .as_deref()
            .is_some_and(|note| !note.is_empty()),
        "the problem says what to do"
    );
    assert!(store.exists(), "the quarantine is written to its store");

    let huge = dir.path().join("assets").join("huge.png");
    let stamp = QuarantineKey::Stamp {
        len: 300 * MIB,
        modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
    };
    refusals.refuse(
        SecurityRefusal {
            path: huge.clone(),
            trust: Trust::Project,
            layer: Layer::File,
            reason: SecurityReason::TooLarge {
                size: 300 * MIB,
                cap: 256 * MIB,
            },
        },
        Some(stamp),
    );

    let escape = SecurityRefusal {
        path: PathBuf::from("assets/../../secret.png"),
        trust: Trust::Project,
        layer: Layer::File,
        reason: SecurityReason::Escape,
    };
    refusals.refuse(escape.clone(), Some(QuarantineKey::Content([9; 32])));
    assert!(
        !refusals.quarantined().contains(&escape),
        "an escape is never stored"
    );
    assert!(
        security(&problems)
            .iter()
            .any(|problem| problem.module == Some(escape.path.display().to_string())),
        "an escape is reported"
    );

    let reread = Refusals::new(None, Some(store.clone()));
    assert!(
        reread.is_quarantined(&liar, &content),
        "read back from the store"
    );
    assert!(
        reread.is_quarantined(&huge, &stamp),
        "a stamp read back too"
    );
    assert!(!reread.quarantined().contains(&escape));

    refusals.release(&liar);
    assert!(!refusals.is_quarantined(&liar, &content));
    assert!(!refusals.quarantined().contains(&wrong_type));
    assert!(
        !security(&problems)
            .iter()
            .any(|problem| problem.module == Some(liar.display().to_string())),
        "a released file's problem is cleared"
    );
    let after_release = Refusals::new(None, Some(store));
    assert!(
        !after_release.is_quarantined(&liar, &content),
        "the release is written to the store"
    );
    assert!(after_release.is_quarantined(&huge, &stamp));
}

/// **A quarantined file is read again once it changes**: its content
/// quarantined by its BLAKE3 hash is refused `Quarantined` and reported;
/// new bytes read; the old bytes back are refused again.
#[test]
fn a_quarantined_file_is_read_again_once_it_changes() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let refusals = reported_to(&problems);
    let file = at.root.join("textures").join("inside.png");
    refusals.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let guard = FileGuard::new(&at.root, Trust::Project, Arc::clone(&refusals));

    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined
    );

    fs::write(&file, b"new bytes").expect("writes");
    assert_eq!(
        guard
            .read("textures/inside.png")
            .expect("a quarantined file whose content changed reads"),
        b"new bytes"
    );

    fs::write(&file, b"inside bytes").expect("writes");
    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined,
        "the content quarantined, back"
    );
}

/// **A file whose bytes lie about its type is refused**: through a guarded
/// file loader, a `.png` holding text fails to load, is refused as the
/// wrong type (`texture`), quarantined and reported through the service's
/// refusals; a `.png` holding a PNG loads.
#[test]
fn a_file_whose_bytes_lie_about_its_type_is_refused() {
    use crate::asset::{AssetService, FileLoader, IndexBuilder, TextureDecoder};
    use khora_core::asset::AssetUUID;
    use khora_core::renderer::api::resource::CpuTexture;

    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    fs::create_dir_all(root.join("textures")).expect("the folder is made");
    fs::write(
        root.join("textures").join("liar.png"),
        b"this is a note, not an image\n",
    )
    .expect("writes");
    image::RgbaImage::new(2, 2)
        .save(root.join("textures").join("honest.png"))
        .expect("a PNG is written");

    let problems = Arc::new(Problems::default());
    let refusals = reported_to(&problems);
    let index = IndexBuilder::new(&root)
        .build_index_bytes()
        .expect("the folder indexes");
    let loader = FileLoader::guarded(FileGuard::new(&root, Trust::Project, Arc::clone(&refusals)));
    let mut service = AssetService::new(
        &index,
        Box::new(loader),
        Arc::new(khora_telemetry::MetricsRegistry::new()),
        None,
    )
    .expect("a service over the index");
    service.register_decoder::<CpuTexture>("texture", TextureDecoder);
    service.set_refusals(Arc::clone(&refusals));

    service
        .load::<CpuTexture>(&AssetUUID::new_v5("textures/honest.png"))
        .expect("a PNG holding a PNG loads");
    assert!(service
        .load::<CpuTexture>(&AssetUUID::new_v5("textures/liar.png"))
        .is_err());

    let wrong_type = refusals.quarantined().into_iter().find(|refusal| {
        refusal.path.ends_with("liar.png")
            && matches!(&refusal.reason, SecurityReason::WrongType { .. })
    });
    let wrong_type = wrong_type.expect("the liar is refused as the wrong type, and quarantined");
    assert_eq!(
        wrong_type.reason,
        SecurityReason::WrongType {
            expected: "texture".to_owned()
        }
    );
    assert!(
        security(&problems).iter().any(|problem| {
            problem.severity == ProblemSeverity::Error
                && problem
                    .module
                    .as_deref()
                    .is_some_and(|module| module.ends_with("liar.png"))
        }),
        "the refusal is reported: {:?}",
        security(&problems)
    );
    assert!(
        !refusals
            .quarantined()
            .iter()
            .any(|refusal| refusal.path.ends_with("honest.png")),
        "the honest file is not refused"
    );
}

/// The decoder registry answers for the decoder of a type, and for none.
#[test]
fn the_decoder_registry_sniffs_with_the_decoder_of_a_type() {
    use crate::asset::{DecoderRegistry, TextureDecoder};
    use khora_core::renderer::api::resource::CpuTexture;

    let mut registry = DecoderRegistry::new(Arc::new(khora_telemetry::MetricsRegistry::new()));
    registry.register::<CpuTexture>("texture", TextureDecoder);

    assert_eq!(registry.sniff("texture", sniffs::PNG), Some(true));
    assert_eq!(registry.sniff("texture", sniffs::TEXT), Some(false));
    assert_eq!(registry.sniff("nothing", sniffs::PNG), None);
}

/// Each built-in decoder knows the bytes its files start with.
mod sniffs {
    use crate::asset::decoders::audio::{SymphoniaDecoder, WavDecoder};
    use crate::asset::decoders::mesh::{
        GltfDecoder, MeshDispatcher, NoOpResourceResolver, ObjDecoder,
    };
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
}

/// **Every read goes through the guard**: a glTF buffer or image, a script,
/// a shader overlay — each refused when it leaves its root, by `..` or
/// through a link inside it, or is over its cap.
mod every_read_goes_through_the_guard {
    use std::fs;
    use std::sync::Arc;

    use khora_core::problems::Problems;

    use super::{layout, link_dir, reported_to, security, sized, MIB};
    use crate::asset::decoders::mesh::{FileSystemResolver, GltfResourceResolver};
    use crate::script::compile::DiskLoader;
    use crate::systems::shader_hot_reload::overlay_source;
    use khora_script::modules::SourceLoader;

    #[test]
    fn a_gltf_buffer_or_image_leaving_the_assets_root_is_refused() {
        let at = layout();
        fs::create_dir_all(at.root.join("models")).expect("the folder is made");
        fs::write(at.root.join("models").join("mesh.bin"), b"buffer").expect("writes");
        fs::write(at.elsewhere.join("secret.bin"), b"secret").expect("writes");
        let resolver = FileSystemResolver::new(&at.root);

        assert_eq!(
            resolver
                .resolve_buffer("models/mesh.bin")
                .expect("a buffer under the root reads"),
            b"buffer"
        );
        assert!(resolver.resolve_buffer("../elsewhere/secret.bin").is_err());
        assert!(resolver.resolve_image("../elsewhere/secret.png").is_err());
        assert!(resolver
            .resolve_buffer(&at.elsewhere.join("secret.bin").display().to_string())
            .is_err());

        if link_dir(&at.elsewhere, &at.root.join("models").join("linked")) {
            assert!(
                resolver.resolve_buffer("models/linked/secret.bin").is_err(),
                "a link inside the root resolving outside, past the lexical URI check"
            );
            assert!(resolver.resolve_image("models/linked/secret.png").is_err());
        }
    }

    #[test]
    fn a_script_leaving_its_root_or_over_its_cap_is_refused() {
        let at = layout();
        let scripts = at.root.join("scripts");
        fs::create_dir_all(&scripts).expect("the folder is made");
        fs::write(scripts.join("guard.erg"), "behavior Guard {}\n").expect("writes");
        fs::write(at.elsewhere.join("secret.erg"), "behavior Secret {}\n").expect("writes");
        sized(&scripts.join("long.erg"), 64 * MIB + 1);
        let loader = DiskLoader::new(&scripts);

        assert!(
            loader.load("guard.erg").is_some(),
            "a script under its root"
        );
        assert!(loader.load("../../elsewhere/secret.erg").is_none());
        assert!(loader
            .load(&at.elsewhere.join("secret.erg").display().to_string())
            .is_none());
        assert!(loader.load("long.erg").is_none(), "a script over its cap");

        if link_dir(&at.elsewhere, &scripts.join("linked")) {
            assert!(
                loader.load("linked/secret.erg").is_none(),
                "a link inside the root resolving outside"
            );
        }
    }

    #[test]
    fn a_shader_overlay_leaving_its_root_or_over_its_cap_is_refused() {
        let at = layout();
        let lib = at.root.join("shaders").join("lib").join("std");
        fs::create_dir_all(&lib).expect("the folder is made");
        fs::write(lib.join("camera.wgsl"), "// camera\n").expect("writes");
        fs::write(at.elsewhere.join("evil.wgsl"), "// evil\n").expect("writes");
        sized(&lib.join("huge.wgsl"), 64 * MIB + 1);
        let problems = Arc::new(Problems::default());
        let refusals = reported_to(&problems);

        assert_eq!(
            overlay_source(&at.root, "shaders/lib/std/camera.wgsl", &refusals).as_deref(),
            Some("// camera\n")
        );
        assert_eq!(
            overlay_source(&at.root, "../elsewhere/evil.wgsl", &refusals),
            None
        );
        assert_eq!(
            overlay_source(&at.root, "shaders/lib/std/huge.wgsl", &refusals),
            None,
            "an overlay over its cap"
        );
        assert!(!security(&problems).is_empty(), "the refusals are reported");

        if link_dir(&at.elsewhere, &at.root.join("shaders").join("linked")) {
            assert_eq!(
                overlay_source(&at.root, "shaders/linked/evil.wgsl", &refusals),
                None,
                "a link inside the root resolving outside"
            );
        }
    }
}

/// **Two runs sharing one store keep each other's quarantine**: two editors
/// open at once (or the editor and a sandbox, both a developer's) read the
/// same `~/.khora/quarantine.ron` at boot, then each quarantines a file of
/// its own. The store ends holding both: a refusal written by one is not
/// erased by the other's next write.
#[test]
fn two_runs_sharing_a_store_keep_each_others_quarantine() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let first = Refusals::new(None, Some(store.clone()));
    let second = Refusals::new(None, Some(store.clone()));
    let refusal_of = |name: &str| SecurityRefusal {
        path: dir.path().join("assets").join(name),
        trust: Trust::Project,
        layer: Layer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    };

    first.refuse(refusal_of("one.png"), Some(QuarantineKey::Content([1; 32])));
    second.refuse(refusal_of("two.png"), Some(QuarantineKey::Content([2; 32])));

    let next_run = Refusals::new(None, Some(store));
    let mut quarantined: Vec<PathBuf> = (next_run.quarantined().into_iter())
        .map(|refusal| refusal.path)
        .collect();
    quarantined.sort();
    assert_eq!(
        quarantined,
        vec![
            dir.path().join("assets").join("one.png"),
            dir.path().join("assets").join("two.png"),
        ],
        "the first run's quarantine survives the second run's write"
    );
}

/// **A file refused for its size is no longer reported once it fits**: a
/// script over its strict cap is refused `TooLarge` and quarantined by its
/// stamp; made smaller, as the remedy asks, it reads — and its refusal
/// leaves the problems and the store, rather than being reported at every
/// boot for a file that now loads.
#[test]
fn a_file_refused_for_its_size_is_no_longer_reported_once_it_fits() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let store = dir.path().join("khora").join("quarantine.ron");
    let file = root.join("scripts").join("long.erg");
    sized(&file, 16 * MIB + 1);
    let problems = Arc::new(Problems::default());
    let refusals = Arc::new(Refusals::new(
        Some(Arc::clone(&problems)),
        Some(store.clone()),
    ));
    let guard = FileGuard::new(&root, Trust::Restricted, Arc::clone(&refusals));

    assert!(matches!(
        refused(guard.read("scripts/long.erg")).reason,
        SecurityReason::TooLarge { .. }
    ));
    assert!(!security(&problems).is_empty(), "the refusal is reported");

    fs::write(&file, "behavior Guard {}\n").expect("the script is made smaller");
    assert_eq!(
        guard
            .read("scripts/long.erg")
            .expect("the script, made smaller, reads"),
        b"behavior Guard {}\n"
    );

    let named = |problem: &Problem| {
        problem
            .module
            .as_deref()
            .is_some_and(|module| module.ends_with("long.erg"))
    };
    assert!(
        !security(&problems).iter().any(named),
        "a file that now reads is no longer reported refused: {:?}",
        security(&problems)
    );
    assert!(
        Refusals::new(None, Some(store)).quarantined().is_empty(),
        "nor kept in the store, to be reported again at the next boot"
    );
}

/// **A quarantined file is refused through any spelling of its root**: the
/// quarantine names a file, not the way its root was typed. The same
/// project opened as `<dir>/assets` in one run and `<dir>/elsewhere/../assets`
/// in the next reads the same file, whose quarantined content is refused.
#[test]
fn a_quarantined_file_is_refused_through_another_spelling_of_its_root() {
    let at = layout();
    let refusals = in_memory();
    refusals.refuse(
        SecurityRefusal {
            path: at.root.join("textures").join("inside.png"),
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let respelled = at.elsewhere.join("..").join("assets");
    let guard = FileGuard::new(&respelled, Trust::Project, Arc::clone(&refusals));

    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined,
        "the quarantined content, read through {}",
        respelled.display()
    );
}

/// A refusal of the file `name` under `dir/assets`, as a texture decoder
/// refuses one.
fn wrong_type_at(dir: &Path, name: &str) -> SecurityRefusal {
    SecurityRefusal {
        path: dir.join("assets").join(name),
        trust: Trust::Project,
        layer: Layer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    }
}

/// **A release by one run sticks though another run sharing the store held
/// the file**: two runs boot over one store holding a quarantined file; the
/// first releases it; the second then quarantines another file. The store
/// must not hold the released file again — the second run's write may keep
/// what others quarantined, not bring back what they released.
#[test]
fn a_release_sticks_when_another_run_sharing_the_store_writes() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let released = wrong_type_at(dir.path(), "released.png");
    let key = QuarantineKey::Content([1; 32]);
    Refusals::new(None, Some(store.clone())).refuse(released.clone(), Some(key));

    let first = Refusals::new(None, Some(store.clone()));
    let second = Refusals::new(None, Some(store.clone()));
    first.release(&released.path);
    second.refuse(
        wrong_type_at(dir.path(), "other.png"),
        Some(QuarantineKey::Content([2; 32])),
    );

    assert!(
        !Refusals::new(None, Some(store)).is_quarantined(&released.path, &key),
        "the file the first run released is back in the store after the second run's write"
    );
}

/// **A file quarantined by another run after this one released it stays in
/// the store**: the first run releases a file; the second, sharing the
/// store, then refuses that file's new content; the first's next write, for
/// another file, must keep the second's quarantine — a release covers what
/// was released, not what another run refused since.
#[test]
fn a_file_quarantined_by_another_run_after_a_release_here_stays_in_the_store() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let first = Refusals::new(None, Some(store.clone()));
    let second = Refusals::new(None, Some(store.clone()));
    let file = wrong_type_at(dir.path(), "liar.png");

    first.refuse(file.clone(), Some(QuarantineKey::Content([1; 32])));
    first.release(&file.path);
    let refused_since = QuarantineKey::Content([2; 32]);
    second.refuse(file.clone(), Some(refused_since));
    first.refuse(
        wrong_type_at(dir.path(), "other.png"),
        Some(QuarantineKey::Content([3; 32])),
    );

    assert!(
        Refusals::new(None, Some(store)).is_quarantined(&file.path, &refused_since),
        "the second run's quarantine of the file is dropped by the first run's write"
    );
}

/// **A file read with another content is no longer reported**: its content
/// quarantined, the file reads once it holds other bytes; it leaves the
/// problems and `quarantined()`, though its old content is still refused
/// and reported again when it comes back.
#[test]
fn a_file_read_with_another_content_is_no_longer_reported() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let refusals = reported_to(&problems);
    let file = at.root.join("textures").join("inside.png");
    refusals.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let guard = FileGuard::new(&at.root, Trust::Project, Arc::clone(&refusals));

    fs::write(&file, b"new bytes").expect("writes");
    guard
        .read("textures/inside.png")
        .expect("the new content reads");
    assert!(refusals.quarantined().is_empty());
    assert!(security(&problems).is_empty(), "{:?}", security(&problems));

    fs::write(&file, b"inside bytes").expect("writes");
    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined
    );
    assert_eq!(refusals.quarantined().len(), 1);
    assert_eq!(security(&problems).len(), 1, "{:?}", security(&problems));
}

/// **Refusals handed over keep a file that reads again unreported**: a
/// guard made before the run's refusals were known quarantined a file's
/// content, then read the file with other content — it is no longer
/// reported. Handed over to the run's refusals, the file must stay
/// unreported there, while its old content stays refused.
#[test]
fn refusals_handed_over_keep_a_file_that_reads_again_unreported() {
    let at = layout();
    let former = in_memory();
    let file = at.root.join("textures").join("inside.png");
    former.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let guard = FileGuard::new(&at.root, Trust::Project, Arc::clone(&former));
    fs::write(&file, b"new bytes").expect("writes");
    guard
        .read("textures/inside.png")
        .expect("the new content reads");
    assert!(former.quarantined().is_empty());

    let problems = Arc::new(Problems::default());
    let run = reported_to(&problems);
    former.hand_over_to(Arc::clone(&run));

    assert!(
        run.quarantined().is_empty(),
        "a file that reads with another content is quarantined again by the hand-over: {:?}",
        run.quarantined()
    );
    assert!(
        security(&problems).is_empty(),
        "and reported again: {:?}",
        security(&problems)
    );
}

/// **A store written before entries named their real path reads**: an
/// entry with only its refusal and key is quarantined by the refusal's path,
/// and survives the next write.
#[test]
fn a_store_from_before_real_paths_reads() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    fs::create_dir_all(store.parent().expect("a folder")).expect("the folder is made");
    let old = wrong_type_at(dir.path(), "old.png");
    let bytes = vec!["7"; 32].join(", ");
    fs::write(
        &store,
        format!(
            "[(refusal: (path: {:?}, trust: Project, layer: Decoder, \
             reason: WrongType(expected: \"texture\")), key: Content(({bytes}))),]",
            old.path.display().to_string()
        ),
    )
    .expect("writes");
    let key = QuarantineKey::Content([7; 32]);

    let refusals = Refusals::new(None, Some(store.clone()));
    assert!(refusals.is_quarantined(&old.path, &key));
    assert_eq!(refusals.quarantined(), vec![old.clone()]);

    refusals.refuse(
        wrong_type_at(dir.path(), "new.png"),
        Some(QuarantineKey::Content([8; 32])),
    );
    assert!(Refusals::new(None, Some(store)).is_quarantined(&old.path, &key));
}

/// The reads a run makes report their refusals to the run's [`Refusals`]:
/// what the project's script compile and the glTF reads of an asset service
/// refuse reaches the run's problems under `Security`.
mod refusals_reach_the_run {
    use std::fs;
    use std::sync::Arc;

    use khora_core::problems::Problems;

    use super::{layout, link_dir, reported_to, security, sized, MIB};
    use crate::trust::{FileGuard, Refusals, Trust};

    /// Whether a problem of the `Security` source names a file ending with
    /// `name`.
    fn reports(problems: &Problems, name: &str) -> bool {
        security(problems).iter().any(|problem| {
            problem
                .module
                .as_deref()
                .is_some_and(|module| module.ends_with(name))
        })
    }

    /// **The project's script compile reports its refusals to the run**: a
    /// module over its cap, compiled by a `ProjectCompiler` made from a
    /// runtime holding the run's refusals and problems, is refused by the
    /// guard — and that refusal is reported under `Security`, as every other
    /// read of the run is.
    #[test]
    fn the_projects_script_compile_reports_its_refusals_to_the_run() {
        use crate::script::project_compiler::ProjectCompiler;

        let at = layout();
        let scripts = at.root.join("scripts");
        fs::create_dir_all(&scripts).expect("the folder is made");
        fs::write(scripts.join("guard.erg"), "behavior Guard {}\n").expect("writes");
        sized(&scripts.join("long.erg"), 64 * MIB + 1);

        let problems = Arc::new(Problems::default());
        let mut runtime = khora_core::Runtime::default();
        runtime.resources.insert(Arc::clone(&problems));
        runtime.resources.insert(reported_to(&problems));

        let _ = ProjectCompiler::from_runtime(&runtime).compile_all(&scripts);

        assert!(
            reports(&problems, "long.erg"),
            "the module refused for its size is reported: {:?}",
            security(&problems)
        );
    }

    /// **A service given the run's refusals after its decoders reports its
    /// glTF reads to them**: as a packed game's launcher does — the service
    /// built and its default decoders registered, then the run's refusals set
    /// by the bootstrap — a glTF buffer reached through a link leaving the
    /// assets root is refused, and reported to the run, not to refusals the
    /// service made for itself before.
    #[test]
    fn a_gltf_read_refused_after_the_service_takes_the_runs_refusals_is_reported_to_them() {
        use crate::asset::{AssetService, FileLoader, IndexBuilder};
        use khora_core::asset::AssetUUID;
        use khora_core::renderer::api::gpu_scene::Mesh;

        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        fs::write(at.elsewhere.join("secret.bin"), [0u8; 36]).expect("writes");
        let gltf = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [ { "uri": "linked/secret.bin", "byteLength": 36 } ],
            "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 36 } ],
            "accessors": [ {
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0],
            } ],
            "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 } } ] } ],
            "nodes": [ { "mesh": 0 } ],
            "scenes": [ { "nodes": [0] } ],
            "scene": 0,
        });
        fs::write(
            models.join("scene.gltf"),
            serde_json::to_vec(&gltf).expect("the glTF serialises"),
        )
        .expect("writes");
        assert!(
            link_dir(&at.elsewhere, &models.join("linked")),
            "this system makes a link (a junction on Windows)"
        );

        let problems = Arc::new(Problems::default());
        let run: Arc<Refusals> = reported_to(&problems);
        let index = IndexBuilder::new(&at.root)
            .build_index_bytes()
            .expect("the folder indexes");
        let loader =
            FileLoader::guarded(FileGuard::new(&at.root, Trust::Project, Arc::clone(&run)));
        let mut service = AssetService::new(
            &index,
            Box::new(loader),
            Arc::new(khora_telemetry::MetricsRegistry::new()),
            None,
        )
        .expect("a service over the index");
        service.register_default_decoders(&at.root);
        service.set_refusals(Arc::clone(&run));

        let whole = AssetUUID::new_v5("models/scene.gltf");
        let _ = service.load::<Mesh>(&whole);
        let _ = service.load::<Mesh>(&AssetUUID::sub_asset(whole, "mesh0/prim0"));

        assert!(
            reports(&problems, "secret.bin"),
            "the glTF buffer leaving the root is reported to the run: {:?}",
            security(&problems)
        );
    }
}

/// **A hand-over back to refusals handed over to is ignored**: after `a`
/// hands over to `b`, `b` handing over to `a` would forward every call
/// between them forever; it is refused, and both still answer.
#[test]
fn a_hand_over_back_to_refusals_handed_over_to_is_ignored() {
    let a = in_memory();
    let b = in_memory();
    a.hand_over_to(Arc::clone(&b));
    b.hand_over_to(Arc::clone(&a));

    let file = PathBuf::from("assets/liar.png");
    a.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: Layer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content([3; 32])),
    );
    assert_eq!(a.quarantined().len(), 1, "a forwards to b");
    assert_eq!(b.quarantined().len(), 1, "b holds what a forwarded");
}
