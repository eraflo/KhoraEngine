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

//! The external files a pack build reads: those its containers name and the index does not list.

use anyhow::{anyhow, Result};
use khora_core::asset::{asset_key, AssetMetadata, AssetSource};
use std::collections::{BTreeMap, HashSet};

use super::super::{external_files_of, LeftOutFile, LeftOutReason};
use crate::trust::{FileGuard, FileKind, ReadError};

/// Returns the external files the containers of `metadata` read that the index does not list, sorted, and those left out.
///
/// Each is paired with the cap class of its first container, which it is read
/// under. A container's external file (a glTF's `.bin`) is no asset, so the
/// index leaves it out; the pack carries it in its external-files table, by
/// path, where the container's resolver finds it. A file the index lists (a
/// glTF's texture) is packed as the asset it is. A container whose bytes cannot
/// be read or listed adds none, logged: reading it as an asset fails the
/// build. A file `guard` does not resolve to one under its root is left out:
/// missing, or leaving it by `..` or through a link, its refusal reported to
/// the guard's refusals. A file listed here can still be refused when it is
/// read, over its container's cap: the build leaves it out the same way.
///
/// # Errors
///
/// When an external file exists but cannot be resolved.
pub(super) fn external_paths(
    guard: &FileGuard,
    metadata: &[AssetMetadata],
) -> Result<ExternalFiles> {
    let indexed: HashSet<String> = metadata
        .iter()
        .map(|entry| asset_key(&entry.source_path))
        .collect();
    let mut external = BTreeMap::new();
    let mut left_out = BTreeMap::new();
    for entry in metadata {
        let Some(AssetSource::Path(rel)) = entry.default_source() else {
            continue;
        };
        let bytes = match guard.read(rel) {
            Ok(bytes) => bytes,
            Err(e) => {
                log::warn!(
                    "Pack: failed to read '{}' to list its external files: {e}",
                    rel.display()
                );
                continue;
            }
        };
        let files = match external_files_of(entry.kind.name(), &bytes, rel) {
            Ok(files) => files,
            Err(e) => {
                log::warn!(
                    "Pack: listing the external files of '{}' failed: {e:#}",
                    rel.display()
                );
                continue;
            }
        };
        let kind = FileKind::of_kind(entry.kind);
        for file in files {
            if indexed.contains(&file)
                || external.contains_key(&file)
                || left_out.contains_key(&file)
            {
                continue;
            }
            let reason = match guard.resolve(&file) {
                Ok(real) if real.is_file() => {
                    external.insert(file, kind);
                    continue;
                }
                Ok(_) => LeftOutReason::Missing,
                Err(ReadError::Refused(refusal)) => LeftOutReason::Refused(refusal.reason),
                Err(error) if error.is_missing() => LeftOutReason::Missing,
                Err(error) => {
                    return Err(anyhow!(
                        "Pack: '{file}', read by '{}', cannot be resolved: {error}",
                        rel.display()
                    ))
                }
            };
            let left = LeftOutFile {
                path: file.clone(),
                reason,
            };
            log::warn!(
                "Pack: {left}, read by '{}': it is not packed",
                rel.display()
            );
            left_out.insert(file, left);
        }
    }
    Ok(ExternalFiles {
        to_pack: external.into_iter().collect(),
        left_out: left_out.into_values().collect(),
    })
}

/// The external files a pack build reads, and those it leaves out.
pub(super) struct ExternalFiles {
    /// Each file to pack, sorted, with the cap class it is read under.
    pub(super) to_pack: Vec<(String, FileKind)>,
    /// Each file left out, sorted.
    pub(super) left_out: Vec<LeftOutFile>,
}
