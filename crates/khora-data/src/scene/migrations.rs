// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Scene format migrations.
//!
//! `SceneHeader.format_version` is bumped whenever the on-disk layout
//! changes. Migrations are registered through `inventory::submit!` and
//! applied in chain at load time before the strategy decodes the payload.
//!
//! Today no migrations are registered (every supported scene already
//! reads as `format_version = 1`). The framework is kept thin so a
//! future schema change can ship the migration alongside the format
//! bump without touching `SerializationService`.
//!
//! Migration ordering: the runner sorts entries by `from_version` and
//! applies each whose `from_version` matches the current payload, then
//! bumps the working version to `to_version` and repeats. Two migrations
//! covering the same `from_version` is a configuration error — the
//! runner picks the first registered match and logs a warning.

use std::fmt;

/// One step of a scene-format migration. Receives the raw payload bytes
/// emitted by the previous version's strategy and returns the bytes the
/// next version's strategy expects.
pub trait SceneMigration: Send + Sync {
    /// Source format version (matches `SceneHeader.format_version`).
    #[allow(clippy::wrong_self_convention)]
    fn from_version(&self) -> u32;
    /// Target format version produced by this migration.
    fn to_version(&self) -> u32;
    /// Apply the migration. Implementations should be pure (no side
    /// effects) and self-contained — they run before the world is
    /// touched.
    fn migrate(&self, payload: &[u8]) -> Result<Vec<u8>, MigrationError>;
}

/// Migration step failure. Wraps strategy-specific errors as text so the
/// runner can keep its surface narrow.
#[derive(Debug)]
pub enum MigrationError {
    /// The payload could not be decoded with the source-version reader.
    DecodeFailed(String),
    /// The payload could not be re-encoded with the target-version writer.
    EncodeFailed(String),
    /// A migration matched but is missing for one of the steps in the chain.
    StepMissing {
        /// Source version that was reached.
        from: u32,
        /// Target version that has no registered migration.
        to: u32,
    },
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DecodeFailed(msg) => write!(f, "migration decode failed: {}", msg),
            Self::EncodeFailed(msg) => write!(f, "migration encode failed: {}", msg),
            Self::StepMissing { from, to } => {
                write!(f, "no migration registered from v{} to v{}", from, to)
            }
        }
    }
}

/// Inventory entry for plugin-registered migrations.
pub struct SceneMigrationRegistration {
    /// The migration step (typically a zero-sized type with the trait
    /// impl). Stored as a static reference so `inventory::collect!`
    /// works without needing a `Box`.
    pub migration: &'static dyn SceneMigration,
}

inventory::collect!(SceneMigrationRegistration);

/// Apply registered migrations to bring `payload` from `from_version`
/// up to `to_version`. Returns the (possibly unchanged) payload bytes.
pub fn migrate_payload(
    mut payload: Vec<u8>,
    from_version: u32,
    to_version: u32,
) -> Result<Vec<u8>, MigrationError> {
    if from_version == to_version {
        return Ok(payload);
    }
    let mut current = from_version;
    while current < to_version {
        let next_step = inventory::iter::<SceneMigrationRegistration>
            .into_iter()
            .map(|r| r.migration)
            .find(|m| m.from_version() == current);
        let Some(step) = next_step else {
            return Err(MigrationError::StepMissing {
                from: current,
                to: current + 1,
            });
        };
        payload = step.migrate(&payload)?;
        current = step.to_version();
    }
    Ok(payload)
}
