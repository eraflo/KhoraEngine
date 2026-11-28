// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! A pack's integrity manifest (`manifest.bin`): each asset's BLAKE3 hash and size.
//!
//! See the book: concepts/trust.md, "Signed packs".

use anyhow::{anyhow, Context, Result};
use khora_core::asset::AssetUUID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{ExternalFileRecord, ManifestEntry};

/// In-memory manifest for fast `verify(uuid, &bytes)` lookups.
///
/// It records every asset entry of the pack by uuid, and every external file
/// of a container by path, so the seal covers the bytes of both.
#[derive(Debug, Default, Clone)]
pub struct PackManifest {
    by_uuid: HashMap<AssetUUID, ManifestEntry>,
    external_files: HashMap<String, ExternalFileRecord>,
}

impl PackManifest {
    /// Returns an empty manifest, filled through [`Self::insert`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the BLAKE3 digest and size of `bytes` under `uuid`.
    pub fn insert(&mut self, uuid: AssetUUID, bytes: &[u8]) {
        let hash: [u8; 32] = *blake3::hash(bytes).as_bytes();
        self.by_uuid.insert(
            uuid,
            ManifestEntry {
                uuid,
                blake3: hash,
                size: bytes.len() as u64,
            },
        );
    }

    /// Records the BLAKE3 digest and size of `bytes` as the external file at `path`.
    pub fn insert_external_file(&mut self, path: &str, bytes: &[u8]) {
        self.external_files.insert(
            path.to_owned(),
            ExternalFileRecord {
                path: path.to_owned(),
                blake3: *blake3::hash(bytes).as_bytes(),
                size: bytes.len() as u64,
            },
        );
    }

    /// Encodes the manifest into the on-disk byte vector.
    ///
    /// # Errors
    ///
    /// When the entries cannot be encoded.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut assets: Vec<ManifestEntry> = self.by_uuid.values().cloned().collect();
        // Sorted so two builds of one project encode the same bytes; by digest,
        // a stable total order.
        assets.sort_by_key(|a| a.blake3);
        let mut external_files: Vec<ExternalFileRecord> =
            self.external_files.values().cloned().collect();
        external_files.sort_by(|a, b| a.path.cmp(&b.path));
        let cfg = bincode::config::standard();
        bincode::serde::encode_to_vec(
            &EncodedManifest {
                assets,
                external_files,
            },
            cfg,
        )
        .map_err(|e| anyhow!("Failed to encode manifest: {}", e))
    }

    /// Decodes the on-disk byte vector into a manifest ready for lookup.
    ///
    /// # Errors
    ///
    /// When `bytes` are not an encoded manifest.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let cfg = bincode::config::standard();
        let (encoded, _): (EncodedManifest, _) = bincode::serde::decode_from_slice(bytes, cfg)
            .context("Failed to decode manifest.bin")?;
        let by_uuid = encoded.assets.into_iter().map(|e| (e.uuid, e)).collect();
        let external_files = (encoded.external_files.into_iter())
            .map(|record| (record.path.clone(), record))
            .collect();
        Ok(Self {
            by_uuid,
            external_files,
        })
    }

    /// Returns `true` if the manifest records no asset and no external file.
    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty() && self.external_files.is_empty()
    }

    /// Returns the number of recorded assets and external files.
    pub fn len(&self) -> usize {
        self.by_uuid.len() + self.external_files.len()
    }

    /// Returns the record of the external file packed at `path`, if any.
    pub fn external_file(&self, path: &str) -> Option<&ExternalFileRecord> {
        self.external_files.get(path)
    }

    /// Returns the record of `uuid`, if any.
    pub fn get(&self, uuid: &AssetUUID) -> Option<&ManifestEntry> {
        self.by_uuid.get(uuid)
    }

    /// Verifies `bytes` against the record of `uuid`.
    ///
    /// # Errors
    /// When the size or the hash differs, or `uuid` has no record.
    pub fn verify(&self, uuid: &AssetUUID, bytes: &[u8]) -> Result<()> {
        let Some(entry) = self.by_uuid.get(uuid) else {
            return Err(anyhow!("Manifest: no record for asset {:?}", uuid));
        };
        check(&format!("{uuid:?}"), entry.size, &entry.blake3, bytes)
    }

    /// Verifies `bytes` against the record of the external file at `path`.
    ///
    /// # Errors
    /// When the size or the hash differs, or `path` has no record.
    pub fn verify_external_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let Some(record) = self.external_files.get(path) else {
            return Err(anyhow!(
                "Manifest: no record for the external file '{path}'"
            ));
        };
        check(&format!("'{path}'"), record.size, &record.blake3, bytes)
    }
}

/// `manifest.bin` as written: its asset records, then its external-file records.
#[derive(Serialize, Deserialize)]
struct EncodedManifest {
    assets: Vec<ManifestEntry>,
    external_files: Vec<ExternalFileRecord>,
}

/// Checks that `bytes` are of `size` and hash to `blake3`.
fn check(what: &str, size: u64, blake3: &[u8; 32], bytes: &[u8]) -> Result<()> {
    if size != bytes.len() as u64 {
        return Err(anyhow!(
            "Manifest: size mismatch for {what} (expected {size}, got {})",
            bytes.len()
        ));
    }
    if blake3::hash(bytes).as_bytes() != blake3 {
        return Err(anyhow!(
            "Manifest: BLAKE3 mismatch for {what} (corrupted or tampered)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_encode_decode() {
        let mut m = PackManifest::new();
        m.insert(AssetUUID::new_v5("a.png"), b"AAAA");
        m.insert(AssetUUID::new_v5("b.png"), b"BBBB");
        let bytes = m.encode().unwrap();
        let m2 = PackManifest::decode(&bytes).unwrap();
        assert_eq!(m.len(), m2.len());
        for (uuid, entry) in &m.by_uuid {
            let other = m2.get(uuid).unwrap();
            assert_eq!(other.blake3, entry.blake3);
            assert_eq!(other.size, entry.size);
        }
    }

    #[test]
    fn verify_accepts_correct_bytes_and_rejects_corruption() {
        let mut m = PackManifest::new();
        let uuid = AssetUUID::new_v5("foo");
        m.insert(uuid, b"PAYLOAD");
        m.verify(&uuid, b"PAYLOAD").unwrap();
        assert!(m.verify(&uuid, b"DIFFERENT").is_err());
        assert!(m.verify(&uuid, b"PAYLOAD-EXTRA").is_err()); // size mismatch
    }

    /// **An external file is recorded by its path**, survives encoding, and
    /// verifies its own bytes only.
    #[test]
    fn an_external_file_is_recorded_and_verified_by_its_path() {
        let mut m = PackManifest::new();
        m.insert_external_file("models/a.bin", b"BUFFER");
        let m = PackManifest::decode(&m.encode().unwrap()).unwrap();
        let record = m
            .external_file("models/a.bin")
            .expect("recorded by its path");
        assert_eq!(record.size, 6);
        assert_eq!(m.len(), 1);
        m.verify_external_file("models/a.bin", b"BUFFER").unwrap();
        assert!(m.verify_external_file("models/a.bin", b"BUFFEr").is_err());
        assert!(m.verify_external_file("models/b.bin", b"BUFFER").is_err());
        assert!(m.external_file("models/b.bin").is_none());
    }

    #[test]
    fn verify_unknown_uuid_errors() {
        let m = PackManifest::new();
        let unknown = AssetUUID::new_v5("nope");
        assert!(m.verify(&unknown, b"x").is_err());
    }
}
