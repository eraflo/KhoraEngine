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

//! **A pack is sealed by the build that made it.**
//! - The build signs `manifest.bin` over `index.bin` with a key kept in a
//!   folder of the user's (made once, never replaced); anything changed in
//!   the manifest, the index or the signature is refused, and so is a pack
//!   signed by another key.
//! - The public key is stamped into the runtime binary, in a slot found by a
//!   magic the binary holds exactly once; a binary never stamped carries no
//!   key.
//!
//! Every signing key here is made in a temporary folder: no test reads or
//! writes the user's home folder.

use std::path::Path;

use khora_core::asset::AssetUUID;
use khora_io::asset::pack_key::{slot_magic, stamp_pack_key, stamped_pack_key};
use khora_io::asset::signing::{PackKey, PackSealError, PackSigner, SignedManifest};
use khora_io::asset::{PackBuilder, PackManifest};

/// The files the manifests here record.
const FILES: &[(&str, &[u8])] = &[
    ("scenes/default.kscene", b"the default scene"),
    ("textures/a.png", b"a texture's bytes"),
];

/// The bytes of an `index.bin` a manifest is signed over. The signature
/// does not read them: any bytes stand for one.
const INDEX: &[u8] = b"the bytes of an index.bin";

/// A manifest recording [`FILES`].
fn manifest() -> PackManifest {
    let mut manifest = PackManifest::new();
    for (rel, bytes) in FILES {
        manifest.insert(AssetUUID::new_v5(rel), bytes);
    }
    manifest
}

/// A signer whose key is kept in a folder of its own, removed with the
/// returned guard.
fn new_signer() -> (tempfile::TempDir, PackSigner) {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let signer = PackSigner::load_or_create(dir.path()).expect("a signing key is made");
    (dir, signer)
}

/// Whether `manifest` records every file of [`FILES`], with their bytes.
fn records_every_file(manifest: &PackManifest) -> bool {
    manifest.len() == FILES.len()
        && FILES
            .iter()
            .all(|(rel, bytes)| manifest.verify(&AssetUUID::new_v5(rel), bytes).is_ok())
}

/// Every position `needle` starts at in `haystack`.
fn positions(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    (0..=haystack.len() - needle.len())
        .filter(|&at| haystack[at] == needle[0] && &haystack[at..at + needle.len()] == needle)
        .collect()
}

// ---------------------------------------------------------------------------
// Signing and verifying
// ---------------------------------------------------------------------------

/// **A pack signed by the build verifies**: a manifest signed over an index,
/// written as `manifest.bin` and read back, verifies with the signer's
/// public key over the same index and gives back the manifest signed — every
/// file, with its bytes. The signed manifest records the index's BLAKE3.
#[test]
fn a_pack_signed_by_the_build_verifies() {
    let (_keys, signer) = new_signer();

    let signed = signer.sign(&manifest(), INDEX).expect("the manifest signs");
    let written = signed.encode().expect("the signed manifest encodes");
    let read = SignedManifest::decode(&written).expect("manifest.bin reads back");

    assert_eq!(read, signed, "manifest.bin does not read back as written");
    assert_eq!(
        read.index,
        *blake3::hash(INDEX).as_bytes(),
        "the signed manifest records the BLAKE3 of index.bin"
    );
    let verified = read
        .verify(&signer.public_key(), INDEX)
        .expect("a pack signed by the build verifies");
    assert!(
        records_every_file(&verified),
        "the manifest verified is not the one signed"
    );
}

/// **A tampered pack is refused**: one byte of the manifest changed →
/// `BadSignature`; another `index.bin` → `IndexChanged`; another index with
/// its hash put in the signed manifest → `BadSignature`; the signature cut →
/// `Malformed` or `BadSignature`; `manifest.bin` cut short, or a bare
/// unsigned manifest in its place → `Malformed`.
#[test]
fn a_tampered_pack_is_refused() {
    let (_keys, signer) = new_signer();
    let key = signer.public_key();
    let signed = signer.sign(&manifest(), INDEX).expect("the manifest signs");

    let mut flipped = signed.clone();
    let last = flipped.manifest.len() - 1;
    flipped.manifest[last] ^= 0x01;
    assert_eq!(
        flipped.verify(&key, INDEX).err(),
        Some(PackSealError::BadSignature),
        "a manifest changed after signing"
    );

    let other_index: &[u8] = b"the bytes of another index.bin";
    assert_eq!(
        signed.verify(&key, other_index).err(),
        Some(PackSealError::IndexChanged),
        "another index.bin"
    );

    let mut rehashed = signed.clone();
    rehashed.index = *blake3::hash(other_index).as_bytes();
    assert_eq!(
        rehashed.verify(&key, other_index).err(),
        Some(PackSealError::BadSignature),
        "another index.bin with its hash put in the signed manifest"
    );

    let mut cut = signed.clone();
    cut.signature.truncate(cut.signature.len() / 2);
    assert!(
        matches!(
            cut.verify(&key, INDEX).err(),
            Some(PackSealError::Malformed(_) | PackSealError::BadSignature)
        ),
        "a signature cut short"
    );

    let written = signed.encode().expect("the signed manifest encodes");
    assert!(
        matches!(
            SignedManifest::decode(&written[..written.len() / 2]),
            Err(PackSealError::Malformed(_))
        ),
        "manifest.bin cut short"
    );
    let bare = manifest().encode().expect("the bare manifest encodes");
    assert!(
        matches!(
            SignedManifest::decode(&bare),
            Err(PackSealError::Malformed(_))
        ),
        "a bare, unsigned manifest"
    );
}

/// **A pack signed by another key is refused**: verified with the public
/// key of another signer, a signed manifest is `BadSignature`.
#[test]
fn a_pack_signed_by_another_key_is_refused() {
    let (_keys, signer) = new_signer();
    let (_other_keys, other) = new_signer();
    assert_ne!(
        signer.public_key(),
        other.public_key(),
        "two signing folders made one key"
    );

    let signed = signer.sign(&manifest(), INDEX).expect("the manifest signs");

    assert_eq!(
        signed.verify(&other.public_key(), INDEX).err(),
        Some(PackSealError::BadSignature)
    );
}

/// **A signed build writes a manifest over its index**: `manifest.bin`,
/// beside the `index.bin` and `data.pack` the build wrote, is a signed
/// manifest that verifies with the signer's key over that `index.bin` and
/// records every packed file. A build without a signer writes no manifest.
#[test]
fn a_signed_build_writes_a_manifest_over_its_index() {
    let (_keys, signer) = new_signer();
    let project = tempfile::tempdir().expect("a temporary folder");
    for (rel, bytes) in FILES {
        let path = project.path().join(rel);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folders");
        std::fs::write(path, bytes).expect("the file is written");
    }
    let signed_out = tempfile::tempdir().expect("a temporary folder");
    let unsigned_out = tempfile::tempdir().expect("a temporary folder");

    let out = PackBuilder::new(project.path(), signed_out.path())
        .with_signer(&signer)
        .build()
        .expect("the project packs, signed");
    let manifest_path = out
        .manifest_bin
        .clone()
        .expect("a signed build writes a manifest");
    assert_eq!(manifest_path, signed_out.path().join("manifest.bin"));
    let signed = SignedManifest::decode(&std::fs::read(&manifest_path).expect("it reads"))
        .expect("manifest.bin is a signed manifest");
    let index = std::fs::read(&out.index_bin).expect("index.bin reads");
    let verified = signed
        .verify(&signer.public_key(), &index)
        .expect("the manifest verifies over the index written");
    assert!(
        records_every_file(&verified),
        "a packed file is not recorded"
    );

    let bare = PackBuilder::new(project.path(), unsigned_out.path())
        .build()
        .expect("the project packs, unsigned");
    assert_eq!(bare.manifest_bin, None);
    assert!(!unsigned_out.path().join("manifest.bin").exists());
}

// ---------------------------------------------------------------------------
// The signing key
// ---------------------------------------------------------------------------

/// **The signing key is made once and kept**: `load_or_create` in a folder
/// that does not exist yet makes the key and writes `pack.key` there; called
/// again, it reads the same key back. Another folder makes another key.
#[test]
fn the_signing_key_is_made_once_and_kept() {
    let home = tempfile::tempdir().expect("a temporary folder");
    let dir = home.path().join(".khora").join("signing");

    let made = PackSigner::load_or_create(&dir).expect("the key is made");
    assert!(dir.join("pack.key").is_file(), "pack.key is not written");
    let kept = PackSigner::load_or_create(&dir).expect("the key is read back");
    let other =
        PackSigner::load_or_create(&home.path().join("elsewhere")).expect("another key is made");

    assert_eq!(
        made.public_key(),
        kept.public_key(),
        "the key was made again"
    );
    assert_ne!(
        made.public_key(),
        other.public_key(),
        "two folders hold one key: the key is not random"
    );
}

/// **A signing key that does not read is not replaced**: a `pack.key`
/// holding something else than a key is an error, and its bytes are left as
/// they were — it is the user's key.
#[test]
fn a_signing_key_that_does_not_read_is_not_replaced() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let path = dir.path().join("pack.key");
    let corrupt = b"not a PKCS#8 key pair".to_vec();
    std::fs::write(&path, &corrupt).expect("the corrupt key is written");

    assert!(PackSigner::load_or_create(dir.path()).is_err());
    assert_eq!(
        std::fs::read(&path).expect("pack.key reads"),
        corrupt,
        "the user's key file was replaced"
    );
}

/// **Two builds making the first key at once share it**: two callers of
/// `load_or_create` on a folder holding no key yet — two Build Game runs,
/// the first ever on this machine — both get a signer, and the same key: the
/// one that loses the race to write `pack.key` reads the winner's back
/// rather than failing the build. Repeated, so the race is met.
#[test]
fn two_builds_making_the_first_key_at_once_share_it() {
    use std::sync::{Arc, Barrier};

    for round in 0..16 {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let barrier = Arc::new(Barrier::new(2));
        let callers: Vec<_> = (0..2)
            .map(|_| {
                let dir = dir.path().join("signing");
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    PackSigner::load_or_create(&dir)
                        .map(|signer| signer.public_key())
                        .map_err(|error| format!("{error:#}"))
                })
            })
            .collect();
        let keys: Vec<Result<PackKey, String>> = callers
            .into_iter()
            .map(|caller| caller.join().expect("the caller does not panic"))
            .collect();

        let made: Vec<PackKey> = keys
            .iter()
            .map(|key| {
                key.clone().unwrap_or_else(|error| {
                    panic!("round {round}: a build making the first key failed: {error}")
                })
            })
            .collect();
        assert_eq!(made[0], made[1], "round {round}: two keys were made");
        let kept = PackSigner::load_or_create(&dir.path().join("signing"))
            .expect("the key kept reads")
            .public_key();
        assert_eq!(
            kept, made[0],
            "round {round}: the key kept is not the one given"
        );
    }
}

// ---------------------------------------------------------------------------
// The key stamped into the runtime binary
// ---------------------------------------------------------------------------

/// A fake binary: `before`, then one empty slot per `slots`, then `after`.
fn fake_binary(slots: usize) -> Vec<u8> {
    let mut bytes = b"the code before the slot ".to_vec();
    for _ in 0..slots {
        bytes.extend_from_slice(&slot_magic());
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(b" between ");
    }
    bytes.extend_from_slice(b"the code after the slot");
    bytes
}

/// Writes `bytes` as a binary in `dir`, and returns its path.
fn binary_at(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the fake binary is written");
    path
}

/// **Stamping writes the key into its slot**: a binary holding one slot,
/// stamped, holds the key in the 32 bytes after the slot's magic and every
/// other byte as it was. A binary holding no slot, or two, is an error and
/// is left as it was.
#[test]
fn stamping_writes_the_key_into_its_slot() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let magic = slot_magic();
    assert_ne!(magic, [0; 32], "the slot's magic is zeros");
    let key = PackKey(*b"thirty-two bytes of a public key");

    let one = fake_binary(1);
    let path = binary_at(dir.path(), "one", &one);
    stamp_pack_key(&path, &key).expect("a binary with one slot is stamped");
    let stamped = std::fs::read(&path).expect("the stamped binary reads");
    let at = positions(&one, &magic)[0];
    let mut expected = one.clone();
    expected[at + 32..at + 64].copy_from_slice(&key.0);
    assert_eq!(
        stamped, expected,
        "the key is not in its slot, or another byte changed"
    );

    for (name, bytes) in [("none", fake_binary(0)), ("two", fake_binary(2))] {
        let path = binary_at(dir.path(), name, &bytes);
        assert!(
            stamp_pack_key(&path, &key).is_err(),
            "a binary with {name} slot was stamped"
        );
        assert_eq!(
            std::fs::read(&path).expect("the binary reads"),
            bytes,
            "a binary with {name} slot was changed"
        );
    }
}

/// **The binary carries one key slot**: the running test binary, which
/// links the slot, holds its magic exactly once — the stamping code, which
/// searches for it, holds no second copy that a stamp could hit.
#[test]
fn the_binary_carries_one_key_slot() {
    let magic = slot_magic();
    assert_ne!(magic, [0; 32], "the slot's magic is zeros");
    // The slot is linked in: the runtime reads it.
    let _ = stamped_pack_key();
    let exe = std::env::current_exe().expect("the test binary's path");
    let binary = std::fs::read(&exe).expect("the test binary reads");

    assert_eq!(
        positions(&binary, &magic).len(),
        1,
        "the test binary does not hold the slot's magic exactly once"
    );
}

/// **An unstamped binary has no pack key**: a test binary is never stamped,
/// so the slot it carries reads as no key.
#[test]
fn an_unstamped_binary_has_no_pack_key() {
    assert_eq!(stamped_pack_key(), None);
}

/// **A `manifest.bin` is measured before it is read**: one over the bound a
/// manifest may hold is refused as malformed from its size alone — the file
/// is sparse, so reading it whole would take its length in memory — and an
/// absent one is missing.
#[test]
fn a_manifest_is_measured_before_it_is_read() {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let path = dir.path().join("manifest.bin");
    assert_eq!(SignedManifest::read(&path), Err(PackSealError::Missing));

    let file = std::fs::File::create(&path).expect("a manifest file");
    file.set_len((64 << 20) + 1)
        .expect("a manifest over the bound");
    drop(file);
    assert!(
        matches!(SignedManifest::read(&path), Err(PackSealError::Malformed(why)) if why.contains("over")),
        "refused from its size"
    );
}
