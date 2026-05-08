// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Pack-integrity sidecar (`manifest.bin`).
//!
//! When the writer enables manifest emission, every entry in `data.pack`
//! gets a BLAKE3 hash of its **uncompressed** bytes recorded alongside
//! its UUID. The runtime reads `manifest.bin` at boot (when
//! `RuntimeConfig::verify_integrity` is set) and re-hashes each asset on
//! load. Mismatches raise `IntegrityError` so corruption / tampering is
//! detected before the bytes hit a decoder.
//!
//! On-disk layout: bincode-encoded `Vec<ManifestEntry>` — same approach
//! as `index.bin`, distinct file so the runtime can opt out cheaply.

use anyhow::{anyhow, Context, Result};
use khora_core::asset::AssetUUID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One row of `manifest.bin` — pre-computed integrity record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub uuid: AssetUUID,
    /// BLAKE3 hash of the asset's *uncompressed* bytes, as a 32-byte digest.
    pub blake3: [u8; 32],
    /// Uncompressed size in bytes — defensive sanity check (mismatch
    /// indicates a corrupted index even before the hash is computed).
    pub size: u64,
}

/// In-memory manifest for fast `verify(uuid, &bytes)` lookups.
#[derive(Debug, Default, Clone)]
pub struct PackManifest {
    by_uuid: HashMap<AssetUUID, ManifestEntry>,
}

impl PackManifest {
    /// Empty manifest — writer accumulates entries through [`Self::insert`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Hashes `bytes` with BLAKE3, records the digest under `uuid`.
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

    /// Encodes the manifest into the on-disk byte vector.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut entries: Vec<ManifestEntry> = self.by_uuid.values().cloned().collect();
        // Stable order — same property the index relies on for byte
        // determinism across builds. Sorted by BLAKE3 digest because
        // `AssetUUID` doesn't implement `Ord` and the digest is already
        // stable + total.
        entries.sort_by(|a, b| a.blake3.cmp(&b.blake3));
        let cfg = bincode::config::standard();
        bincode::serde::encode_to_vec(&entries, cfg)
            .map_err(|e| anyhow!("Failed to encode manifest: {}", e))
    }

    /// Decodes the on-disk byte vector into a manifest ready for lookup.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let cfg = bincode::config::standard();
        let (entries, _): (Vec<ManifestEntry>, _) = bincode::serde::decode_from_slice(bytes, cfg)
            .context("Failed to decode manifest.bin")?;
        let by_uuid = entries.into_iter().map(|e| (e.uuid, e)).collect();
        Ok(Self { by_uuid })
    }

    /// Returns `true` if the manifest contains no entries.
    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty()
    }

    /// Number of recorded entries.
    pub fn len(&self) -> usize {
        self.by_uuid.len()
    }

    /// Retrieve the recorded hash + size for `uuid`.
    pub fn get(&self, uuid: &AssetUUID) -> Option<&ManifestEntry> {
        self.by_uuid.get(uuid)
    }

    /// Verifies `bytes` against the recorded hash for `uuid`.
    /// Returns `Ok(())` on match, `Err` on mismatch (or if the uuid is
    /// not in the manifest — the caller chose to verify but the record
    /// is missing).
    pub fn verify(&self, uuid: &AssetUUID, bytes: &[u8]) -> Result<()> {
        let Some(entry) = self.by_uuid.get(uuid) else {
            return Err(anyhow!("Manifest: no record for asset {:?}", uuid));
        };
        if entry.size != bytes.len() as u64 {
            return Err(anyhow!(
                "Manifest: size mismatch for {:?} (expected {}, got {})",
                uuid,
                entry.size,
                bytes.len()
            ));
        }
        let actual: [u8; 32] = *blake3::hash(bytes).as_bytes();
        if actual != entry.blake3 {
            return Err(anyhow!(
                "Manifest: BLAKE3 mismatch for {:?} (corrupted or tampered)",
                uuid
            ));
        }
        Ok(())
    }
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

    #[test]
    fn verify_unknown_uuid_errors() {
        let m = PackManifest::new();
        let unknown = AssetUUID::new_v5("nope");
        assert!(m.verify(&unknown, b"x").is_err());
    }
}
