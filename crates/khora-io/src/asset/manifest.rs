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

/// One row of `manifest.bin`: an asset's integrity record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// The asset the record is of.
    pub uuid: AssetUUID,
    /// BLAKE3 hash of the asset's *uncompressed* bytes, as a 32-byte digest.
    pub blake3: [u8; 32],
    /// Uncompressed size in bytes, checked before the hash is computed.
    pub size: u64,
}

/// In-memory manifest for fast `verify(uuid, &bytes)` lookups.
#[derive(Debug, Default, Clone)]
pub struct PackManifest {
    by_uuid: HashMap<AssetUUID, ManifestEntry>,
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

    /// Encodes the manifest into the on-disk byte vector.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut entries: Vec<ManifestEntry> = self.by_uuid.values().cloned().collect();
        // Sorted so two builds of one project encode the same bytes; by digest,
        // a stable total order.
        entries.sort_by_key(|a| a.blake3);
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
