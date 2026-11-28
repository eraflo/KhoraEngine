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

//! Tests of the file-system helpers, one module per helper module.

use super::*;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::temporary::{aside_beside, previous_beside, temporary_beside, temporary_of, Leftover};

mod bounded_read;
mod paths;
mod rename;
mod replace_dir;
mod temporary;
mod walk;
mod write;

/// The names of the entries of `dir`, sorted.
fn listed(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Returns whether file names in `dir` ignore case.
fn ignores_case(dir: &Path) -> bool {
    let probe = dir.join("Case-Probe");
    std::fs::write(&probe, b"").unwrap();
    let ignores = dir.join("case-probe").exists();
    std::fs::remove_file(&probe).unwrap();
    ignores
}
