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

//! Signed packs: the build signs a pack's `manifest.bin` over its
//! `index.bin` with the user's key, and the runtime verifies it with the
//! public key stamped into its binary.
//!
//! A pack is sealed against changes, not against someone who rebuilds or
//! re-stamps the runtime binary.

use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};

use super::PackManifest;

/// An Ed25519 public key: what a runtime verifies a pack with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PackKey(pub [u8; 32]);

/// The user's key pair, signing the packs their builds make.
pub struct PackSigner {
    pair: Ed25519KeyPair,
}

/// The file the key pair is kept in, under its folder.
const KEY_FILE: &str = "pack.key";

/// The largest key file read: a PKCS#8 Ed25519 document is under a hundred
/// bytes.
const MAX_KEY_BYTES: u64 = 4096;

/// What the bytes of `manifest.bin` start with.
const MAGIC: &[u8] = b"KH_SIGNED_MANIFEST_V1\0";

/// The largest signed manifest read.
const MAX_MANIFEST_BYTES: usize = 64 << 20;

/// What a signature is made over, before the index hash and the manifest:
/// a signature of anything else never verifies as a pack's.
const DOMAIN: &[u8] = b"khora pack manifest v1\0";

impl PackSigner {
    /// The key pair at `<dir>/pack.key`, made with the OS random source and
    /// written atomically when the file is absent. A file that exists and
    /// does not read is an error, never replaced: it is the user's key.
    pub fn load_or_create(dir: &Path) -> Result<PackSigner> {
        let path = dir.join(KEY_FILE);
        if path.exists() {
            let len = std::fs::metadata(&path)
                .with_context(|| format!("cannot read {}", path.display()))?
                .len();
            if len > MAX_KEY_BYTES {
                return Err(anyhow!(
                    "{} is {len} bytes: not a pack signing key; it is kept as it is",
                    path.display()
                ));
            }
            let bytes =
                std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
            let pair = Ed25519KeyPair::from_pkcs8(&bytes).map_err(|_| {
                anyhow!(
                    "{} is not a pack signing key; it is kept as it is — move it away to \
                     make another",
                    path.display()
                )
            })?;
            return Ok(PackSigner { pair });
        }
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| anyhow!("the OS random source gave no key"))?;
        match write_new(&path, document.as_ref()) {
            Ok(()) => log::info!("pack signing key made at {}", path.display()),
            // Another build made it first: its key is the user's.
            Err(error) if is_already_there(&error) => return Self::load_or_create(dir),
            Err(error) => return Err(error),
        }
        let pair = Ed25519KeyPair::from_pkcs8(document.as_ref())
            .map_err(|_| anyhow!("the key just made does not read"))?;
        Ok(PackSigner { pair })
    }

    /// The public half of the key pair.
    pub fn public_key(&self) -> PackKey {
        let mut key = [0u8; 32];
        key.copy_from_slice(self.pair.public_key().as_ref());
        PackKey(key)
    }

    /// `manifest`, signed over the `index.bin` whose bytes are `index_bytes`.
    pub fn sign(&self, manifest: &PackManifest, index_bytes: &[u8]) -> Result<SignedManifest> {
        let manifest = manifest.encode()?;
        let index = *blake3::hash(index_bytes).as_bytes();
        let signature = self
            .pair
            .sign(&signed_bytes(&index, &manifest))
            .as_ref()
            .to_vec();
        Ok(SignedManifest {
            manifest,
            index,
            signature,
        })
    }
}

/// Writes `bytes` at `path`, which must not exist: through a file beside it,
/// linked in place only if nothing is there — a key is never replaced.
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    // A name of its own for each write: two builds of one process write
    // apart.
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let write = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.{write}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // The key is the user's alone.
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .with_context(|| format!("cannot write {}", tmp.display()))?;
    }
    let linked = match std::fs::hard_link(&tmp, path) {
        Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
            // A file system without hard links: the file is made in place,
            // still never over one that is there.
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        }
        linked => linked,
    };
    let _ = std::fs::remove_file(&tmp);
    linked.with_context(|| format!("cannot write {}", path.display()))
}

/// Whether `error` says the file to make was there already.
fn is_already_there(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::AlreadyExists)
}

/// What a signature covers: the domain, the index's hash, the manifest.
fn signed_bytes(index: &[u8; 32], manifest: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(DOMAIN.len() + index.len() + manifest.len());
    bytes.extend_from_slice(DOMAIN);
    bytes.extend_from_slice(index);
    bytes.extend_from_slice(manifest);
    bytes
}

/// A pack's `manifest.bin`: its [`PackManifest`] signed over its
/// `index.bin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedManifest {
    /// The [`PackManifest`], encoded.
    pub manifest: Vec<u8>,
    /// The BLAKE3 of the `index.bin` the manifest was signed over.
    pub index: [u8; 32],
    /// The Ed25519 signature.
    pub signature: Vec<u8>,
}

impl SignedManifest {
    /// The bytes of `manifest.bin`, behind a magic and a version
    /// (`KH_SIGNED_MANIFEST_V1`).
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = MAGIC.to_vec();
        let body = bincode::serde::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| anyhow!("cannot encode the signed manifest: {e}"))?;
        bytes.extend_from_slice(&body);
        Ok(bytes)
    }

    /// Reads the `manifest.bin` at `path`: its size checked before it is
    /// read, then [`decode`](Self::decode)d. Absent, it is
    /// [`PackSealError::Missing`].
    pub fn read(path: &Path) -> Result<SignedManifest, PackSealError> {
        let len = match std::fs::metadata(path) {
            Ok(meta) => meta.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PackSealError::Missing)
            }
            Err(error) => return Err(PackSealError::Malformed(error.to_string())),
        };
        if len > MAX_MANIFEST_BYTES as u64 {
            return Err(PackSealError::Malformed(format!(
                "{len} bytes, over the {MAX_MANIFEST_BYTES} a manifest may hold"
            )));
        }
        let bytes = std::fs::read(path).map_err(|e| PackSealError::Malformed(e.to_string()))?;
        Self::decode(&bytes)
    }

    /// Reads `manifest.bin`, bounded; anything else than a signed manifest
    /// is [`PackSealError::Malformed`].
    pub fn decode(bytes: &[u8]) -> Result<SignedManifest, PackSealError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(PackSealError::Malformed(format!(
                "{} bytes, over the {MAX_MANIFEST_BYTES} a manifest may hold",
                bytes.len()
            )));
        }
        let Some(body) = bytes.strip_prefix(MAGIC) else {
            return Err(PackSealError::Malformed(
                "it is not signed — a pack built before packs were signed; build it again"
                    .to_owned(),
            ));
        };
        let config = bincode::config::standard().with_limit::<MAX_MANIFEST_BYTES>();
        let (signed, read): (SignedManifest, usize) =
            bincode::serde::decode_from_slice(body, config)
                .map_err(|e| PackSealError::Malformed(e.to_string()))?;
        if read != body.len() {
            return Err(PackSealError::Malformed(format!(
                "{} bytes after its end",
                body.len() - read
            )));
        }
        Ok(signed)
    }

    /// The manifest, once its signature verifies with `key` over the
    /// `index.bin` whose bytes are `index_bytes`.
    pub fn verify(&self, key: &PackKey, index_bytes: &[u8]) -> Result<PackManifest, PackSealError> {
        // The signature first: nothing of an unverified manifest is read.
        UnparsedPublicKey::new(&ED25519, key.0)
            .verify(&signed_bytes(&self.index, &self.manifest), &self.signature)
            .map_err(|_| PackSealError::BadSignature)?;
        if *blake3::hash(index_bytes).as_bytes() != self.index {
            return Err(PackSealError::IndexChanged);
        }
        PackManifest::decode(&self.manifest).map_err(|e| PackSealError::Malformed(format!("{e:#}")))
    }
}

/// Why a pack's seal was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackSealError {
    /// The pack has no `manifest.bin`.
    #[error("the pack has no manifest.bin: build it again")]
    Missing,
    /// `manifest.bin` is not a signed manifest.
    #[error("manifest.bin is not a signed manifest ({0})")]
    Malformed(String),
    /// `index.bin` is not the one the manifest was signed over.
    #[error("index.bin is not the one the pack was signed over: the pack was changed")]
    IndexChanged,
    /// The signature does not verify with the runtime's key.
    #[error(
        "the pack's signature does not verify with this runtime's key: the pack was changed, \
         or built by another key"
    )]
    BadSignature,
    /// The runtime binary carries no key to verify a pack with.
    #[error("this runtime binary carries no pack key: build the game with Build Game")]
    Unstamped,
}
