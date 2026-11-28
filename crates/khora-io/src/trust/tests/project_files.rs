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

//! The files of a project's root outside `assets/`: each read through a guard
//! over the root, under the cap of its own kind.

use std::fs;
use std::sync::Arc;

use tempfile::TempDir;

use super::super::*;
use super::MIB;

/// A project root holding each of `files`, by path under the root and length.
fn root_with(files: &[(&str, u64)]) -> TempDir {
    let dir = TempDir::new().expect("a temporary folder");
    for (rel, len) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().expect("a folder")).expect("the file's folder");
        fs::File::create(&path)
            .and_then(|file| file.set_len(*len))
            .expect("the file is made");
    }
    dir
}

/// **Each project file has a cap of its own kind**, the strict one no larger.
#[test]
fn each_project_file_has_a_cap_of_its_own_kind() {
    let caps = [
        (FileKind::Manifest, MIB, 256 << 10),
        (FileKind::Registry, 64 * MIB, 16 * MIB),
        (FileKind::GitHead, 4 << 10, 4 << 10),
        (FileKind::GitIgnore, MIB, 256 << 10),
    ];
    for (kind, larger, strict) in caps {
        assert_eq!(size_cap(kind, Trust::Project), larger, "{kind:?}");
        assert_eq!(size_cap(kind, Trust::Restricted), strict, "{kind:?}");
        assert_eq!(
            Limits::of(kind, Trust::Restricted).max_bytes,
            strict,
            "{kind:?}"
        );
    }
    for (file, kind) in [
        (ProjectFile::Manifest, FileKind::Manifest),
        (ProjectFile::IdentityRegistry, FileKind::Registry),
        (ProjectFile::GitHead, FileKind::GitHead),
        (ProjectFile::GitIgnore, FileKind::GitIgnore),
    ] {
        assert_eq!(file.kind(), kind, "{file:?}");
    }
}

/// **A project file is read from its place under the root**: `project.json`,
/// `.khora/asset-registry.ron`, `.git/HEAD`, `.gitignore`.
#[test]
fn a_project_file_is_read_from_its_place_under_the_root() {
    let dir = root_with(&[
        ("project.json", 2),
        (".khora/asset-registry.ron", 3),
        (".git/HEAD", 4),
        (".gitignore", 5),
    ]);
    let guard = FileGuard::new(dir.path(), Trust::Project, Refusals::detached());
    for (file, len) in [
        (ProjectFile::Manifest, 2),
        (ProjectFile::IdentityRegistry, 3),
        (ProjectFile::GitHead, 4),
        (ProjectFile::GitIgnore, 5),
    ] {
        let bytes = guard.read_project_file(file).expect("the file reads");
        assert_eq!(bytes.len(), len, "{file:?}");
    }
}

/// **A project file over its strict cap is refused before the trust is decided**,
/// and the refusal reported; the trusted cap reads it.
#[test]
fn a_project_file_over_its_strict_cap_is_refused_and_reported() {
    let dir = root_with(&[("project.json", (256 << 10) + 1)]);
    let refusals = Arc::new(Refusals::new(None, None));
    let strict = FileGuard::new(dir.path(), Trust::Restricted, Arc::clone(&refusals));
    match strict.read_project_file(ProjectFile::Manifest) {
        Err(ReadError::Refused(refusal)) => {
            assert!(matches!(refusal.reason, SecurityReason::TooLarge { .. }));
            assert!(refusal.path.ends_with("project.json"), "{refusal:?}");
        }
        other => panic!("refused over the strict cap: {other:?}"),
    }
    assert!(
        refusals
            .quarantined()
            .iter()
            .any(|refusal| refusal.path.ends_with("project.json")),
        "the refusal is reported: {:?}",
        refusals.quarantined()
    );
    let trusted = FileGuard::new(dir.path(), Trust::Project, Refusals::detached());
    assert!(trusted.read_project_file(ProjectFile::Manifest).is_ok());
}

/// **A project file that does not exist reads as missing**, and is refused nothing.
#[test]
fn a_missing_project_file_reads_as_missing() {
    let dir = root_with(&[]);
    let refusals = Arc::new(Refusals::new(None, None));
    let guard = FileGuard::new(dir.path(), Trust::Restricted, Arc::clone(&refusals));
    let error = guard
        .read_project_file(ProjectFile::GitHead)
        .expect_err("there is no `.git/HEAD`");
    assert!(error.is_missing(), "{error:?}");
    assert!(refusals.quarantined().is_empty());
}

/// **A file refused for what it holds stays refused until it changes.**
///
/// `refuse_content` reports the refusal at the content layer and quarantines
/// the file: read again unchanged it is refused as quarantined; changed, it reads.
#[test]
fn a_file_refused_for_its_content_stays_refused_until_it_changes() {
    let dir = TempDir::new().expect("a temporary folder");
    fs::write(dir.path().join("project.json"), b"{ not json").expect("writes");
    let refusals = Arc::new(Refusals::new(None, None));
    let guard = FileGuard::new(dir.path(), Trust::Project, Arc::clone(&refusals));
    let bytes = guard
        .read_project_file(ProjectFile::Manifest)
        .expect("it reads");

    let refusal = guard.refuse_content(
        "project.json",
        SecurityReason::Malformed {
            expected: "project manifest".to_owned(),
            why: "expected a value".to_owned(),
        },
        &bytes,
    );

    assert_eq!(refusal.layer, TrustLayer::Content);
    assert_eq!(refusals.quarantined(), vec![refusal]);
    assert!(matches!(
        guard.read_project_file(ProjectFile::Manifest),
        Err(ReadError::Refused(SecurityRefusal {
            reason: SecurityReason::Quarantined,
            ..
        }))
    ));
    fs::write(dir.path().join("project.json"), b"{}").expect("mended");
    assert!(guard.read_project_file(ProjectFile::Manifest).is_ok());
}
