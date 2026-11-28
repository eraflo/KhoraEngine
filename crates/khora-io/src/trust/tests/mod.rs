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

//! The trust boundary, pinned: a read leaving its root, a file over its cap,
//! bytes lying about their type, a refusal reported and quarantined, and every
//! read of a project's files — assets, glTF buffers, scripts, shader overlays —
//! going through the guard.
//!
//! Every folder here is a temporary one: no test reads or writes the user's
//! home folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use tempfile::TempDir;

use super::*;

mod every_read;
mod file_kind;
mod guard;
mod keys_and_releases;
mod labels;
mod limits;
mod project_files;
mod project_trust;
mod quarantine;
mod quarantine_view;
mod sniffs;
mod text;
mod wrong_type;

const MIB: u64 = 1 << 20;

/// A project's assets folder, `<dir>/assets`, holding `textures/inside.png`.
///
/// Beside it, a folder `<dir>/elsewhere` holds `secret.png`, under no root.
struct Layout {
    _dir: TempDir,
    root: PathBuf,
    elsewhere: PathBuf,
    outside: PathBuf,
}

fn layout() -> Layout {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    fs::create_dir_all(root.join("textures")).expect("the assets folder is made");
    fs::write(root.join("textures").join("inside.png"), b"inside bytes").expect("writes");
    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).expect("the outside folder is made");
    let outside = elsewhere.join("secret.png");
    fs::write(&outside, b"secret bytes").expect("writes");
    Layout {
        _dir: dir,
        root,
        elsewhere,
        outside,
    }
}

/// Refusals reported to `problems`, kept in memory only.
fn reported_to(problems: &Arc<Problems>) -> Arc<Refusals> {
    Arc::new(Refusals::new(Some(Arc::clone(problems)), None))
}

/// Refusals kept in memory, reported nowhere.
fn in_memory() -> Arc<Refusals> {
    Arc::new(Refusals::new(None, None))
}

/// What the security source holds.
fn security(problems: &Problems) -> Vec<Problem> {
    problems.of_source(&ProblemSource::Security)
}

/// The refusal a read returned; panics on bytes or an IO error.
fn refused(result: Result<Vec<u8>, ReadError>) -> SecurityRefusal {
    match result {
        Err(ReadError::Refused(refusal)) => refusal,
        Err(ReadError::Io(error)) => panic!("a refusal, not an IO error: {error}"),
        Ok(bytes) => panic!("a refusal, yet {} bytes were read", bytes.len()),
    }
}

/// A file of `len` bytes at `path`, made without writing them.
fn sized(path: &Path, len: u64) {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).expect("the folder is made");
    }
    fs::File::create(path)
        .and_then(|file| file.set_len(len))
        .expect("a file of the size asked for");
}

/// Makes `link` a link to the folder `target`: a symbolic link, or on Windows a
/// junction when a symbolic link needs a privilege the test lacks.
///
/// False when the system makes neither, and the test that needed it skips that
/// part.
fn link_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return true;
        }
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success())
            && link.exists()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        false
    }
}
