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

//! `cargo xtask lint-file`: the per-file convention checks on given files.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use xtask::lint::{hint, lint_files, normalize, root_for};

/// Exit code when every file is clean.
pub const CLEAN: i32 = 0;

/// Exit code when a check found something.
pub const FINDINGS: i32 = 1;

/// Exit code when a file could not be read.
pub const FAILED: i32 = 2;

/// Lints `paths`, prints each finding as `path:line: check: message` on
/// stdout, and returns the exit code: [`CLEAN`], [`FINDINGS`] or [`FAILED`].
///
/// Paths are read relative to the current folder; a file that is not `.rs`,
/// or lies outside `crates/`, `xtask/` and `examples/`, is skipped.
pub fn run(paths: &[PathBuf]) -> i32 {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let absolute: Vec<PathBuf> = paths.iter().map(|path| absolute(path)).collect();
    let Some(first) = absolute.first() else {
        return CLEAN;
    };
    let root = root_for(first);
    let findings = match lint_files(&root, &absolute) {
        Ok(findings) => findings,
        Err(error) => {
            let _ = writeln!(stderr, "lint-file: cannot read {error}");
            return FAILED;
        }
    };
    if findings.is_empty() {
        return CLEAN;
    }
    for finding in &findings {
        let _ = writeln!(stdout, "{finding}");
    }
    let checks: BTreeSet<&str> = findings.iter().map(|finding| finding.check).collect();
    let _ = writeln!(stdout, "\n{} findings. What to do:", findings.len());
    for check in checks {
        let _ = writeln!(stdout, "  {check}: {}", hint(check));
    }
    FINDINGS
}

/// Returns `path` made absolute against the current folder, `.` and `..`
/// resolved without touching the file system.
fn absolute(path: &Path) -> PathBuf {
    let joined = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    normalize(&joined)
}
