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

//! The open project's scripts, mounted while it is open.
//!
//! Opening a project mounts `<project>/assets`; opening another unmounts the
//! first and mounts the second; closing unmounts. While the editor edits,
//! what the scripts declare registers — the registry lists the project's
//! behaviors and components — and what they compiled to waits for Play.

use std::path::{Path, PathBuf};

use khora_sdk::Runtime;

/// Brings the mounted scripts in line with the open project, called each
/// frame with the project open now (`None` when none is).
///
/// `mounted` is the editor's memory of what it mounted, across calls. Returns
/// how many modules compiled when this call mounted a project, `None` when it
/// mounted nothing — the project open is the one already mounted, or none is
/// open.
pub(crate) fn follow_open_project(
    runtime: &Runtime,
    mounted: &mut Option<PathBuf>,
    project: Option<&Path>,
) -> Option<usize> {
    let wanted = project.map(|project| project.join("assets"));
    if *mounted == wanted {
        return None;
    }
    if let Some(previous) = mounted.take() {
        khora_sdk::scripts::unmount(runtime, &previous);
    }
    let assets = wanted?;
    let loaded = khora_sdk::scripts::mount(runtime, &assets);
    *mounted = Some(assets);
    Some(loaded)
}

#[cfg(test)]
mod tests;
