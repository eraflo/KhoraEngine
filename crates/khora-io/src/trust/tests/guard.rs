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

//! The guard, pinned: a read leaving its root, a file over its cap, the trust a file is read at.

use super::*;

/// **A path escaping its root is refused.**
///
/// `..` leaving it, a path through a folder then `..` past the root, an
/// absolute path, and a link inside the root that resolves outside it are each
/// refused as an `Escape`, at the file layer, at the guard's trust, and
/// reported. A file under the root reads its bytes, through a link that stays
/// inside too; a missing file is an IO error, not a refusal.
#[test]
fn a_path_escaping_its_root_is_refused() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let guard = FileGuard::new(&at.root, Trust::Project, reported_to(&problems));

    assert_eq!(
        guard
            .read("textures/inside.png")
            .expect("a file under the root reads"),
        b"inside bytes"
    );

    for escaping in [
        PathBuf::from("../elsewhere/secret.png"),
        PathBuf::from("textures/../../elsewhere/secret.png"),
        at.outside.clone(),
    ] {
        let refusal = refused(guard.read(&escaping));
        assert_eq!(refusal.reason, SecurityReason::Escape, "{escaping:?}");
        assert_eq!(refusal.layer, TrustLayer::File, "{escaping:?}");
        assert_eq!(refusal.trust, Trust::Project, "{escaping:?}");
    }

    assert!(
        matches!(guard.read("textures/missing.png"), Err(ReadError::Io(_))),
        "a missing file is an IO error"
    );
    assert!(
        !security(&problems).is_empty(),
        "every refusal is reported to the problems"
    );

    let out_link = at.root.join("linked");
    if link_dir(&at.elsewhere, &out_link) {
        let refusal = refused(guard.read("linked/secret.png"));
        assert_eq!(
            refusal.reason,
            SecurityReason::Escape,
            "a link inside the root resolving outside it"
        );
    } else {
        log::warn!("this system makes no link: the link escape is not checked");
    }
    let in_link = at.root.join("alias");
    if link_dir(&at.root.join("textures"), &in_link) {
        assert_eq!(
            guard
                .read("alias/inside.png")
                .expect("a link resolving inside the root reads"),
            b"inside bytes"
        );
    }
}

/// **A path is resolved under the guard's rules, without being read**: a file
/// under the root resolves to its real path, one leaving it is refused and
/// reported, a missing one is an IO error.
#[test]
fn a_path_is_resolved_under_the_guards_rules() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let guard = FileGuard::new(&at.root, Trust::Project, reported_to(&problems));

    let real = guard
        .resolve("textures/inside.png")
        .expect("a file under the root resolves");
    assert_eq!(
        real,
        at.root
            .join("textures")
            .join("inside.png")
            .canonicalize()
            .expect("the file exists")
    );
    let refusal = match guard.resolve("../elsewhere/secret.png") {
        Err(ReadError::Refused(refusal)) => refusal,
        other => panic!("a refusal, not {other:?}"),
    };
    assert_eq!(refusal.reason, SecurityReason::Escape);
    assert!(!security(&problems).is_empty(), "the refusal is reported");
    assert!(matches!(
        guard.resolve("textures/missing.png"),
        Err(ReadError::Io(_))
    ));

    if link_dir(&at.elsewhere, &at.root.join("linked")) {
        assert!(
            matches!(
                guard.resolve("linked/secret.png"),
                Err(ReadError::Refused(_))
            ),
            "a link inside the root resolving outside it"
        );
    }
}

/// **A guard holds to its root however the root or the path is named.**
///
/// A folder beside the root whose name begins with the root's, reached as a
/// resolved (verbatim on Windows) absolute path or through a link inside the
/// root, is outside; a root that is itself a link, or named in another case
/// where file names ignore it, reads what is under it and refuses the rest.
#[test]
fn a_guard_holds_to_its_root_however_it_or_the_path_is_named() {
    let at = layout();
    let sibling = at.root.with_file_name("assets-other");
    fs::create_dir_all(&sibling).expect("the sibling folder is made");
    fs::write(sibling.join("secret.png"), b"sibling bytes").expect("writes");
    let guard = FileGuard::new(&at.root, Trust::Project, in_memory());

    let resolved = sibling
        .join("secret.png")
        .canonicalize()
        .expect("the file exists");
    assert_eq!(
        refused(guard.read(&resolved)).reason,
        SecurityReason::Escape
    );
    if link_dir(&sibling, &at.root.join("near")) {
        assert_eq!(
            refused(guard.read("near/secret.png")).reason,
            SecurityReason::Escape,
            "a link to a folder sharing the root's name as a prefix"
        );
    }

    let linked_root = at.root.with_file_name("linked-root");
    if link_dir(&at.root, &linked_root) {
        let through = FileGuard::new(&linked_root, Trust::Project, in_memory());
        assert_eq!(
            through
                .read("textures/inside.png")
                .expect("under a linked root"),
            b"inside bytes"
        );
        assert_eq!(
            refused(through.read("near/secret.png")).reason,
            SecurityReason::Escape
        );
    }

    let other_case = PathBuf::from(at.root.to_string_lossy().to_uppercase());
    if other_case != at.root && other_case.exists() {
        let cased = FileGuard::new(&other_case, Trust::Project, in_memory());
        assert_eq!(
            cased
                .read("textures/inside.png")
                .expect("under a root in another case"),
            b"inside bytes"
        );
        assert_eq!(
            refused(cased.read(&resolved)).reason,
            SecurityReason::Escape
        );
    }
}

/// **A project's guard reads at the trust of the project open**, reporting to
/// the refusals it is given; a folder outside the project named is restricted.
#[test]
fn a_projects_guard_reads_at_the_trust_of_the_project_open() {
    let at = layout();
    let refusals = in_memory();
    let state = ProjectTrustState::new(ProjectTrust::Restricted);
    let restricted = FileGuard::for_project(&at.root, &state, Arc::clone(&refusals));
    assert_eq!(restricted.trust(), Trust::Restricted);
    assert!(Arc::ptr_eq(restricted.refusals(), &refusals));

    state.set_for(&at.root, ProjectTrust::Trusted);
    assert_eq!(
        FileGuard::for_project(&at.root, &state, Arc::clone(&refusals)).trust(),
        Trust::Project
    );
    assert_eq!(
        FileGuard::for_project(&at.elsewhere, &state, refusals).trust(),
        Trust::Restricted,
        "a folder outside the project open"
    );
}

/// **A file over its cap is refused before it is read.**
///
/// Its size, read from the metadata, is over the cap of its kind at the guard's
/// trust — the strict caps under `Restricted` and `Imported`, the larger under
/// `Project`. A file exactly at its cap reads.
#[test]
fn a_file_over_its_cap_is_refused_before_it_is_read() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    sized(&root.join("big.png"), 64 * MIB + 1);
    sized(&root.join("huge.png"), 256 * MIB + 1);
    sized(&root.join("scripts").join("long.erg"), 64 * MIB + 1);
    sized(&root.join("scripts").join("edge.erg"), 16 * MIB);

    for trust in [Trust::Restricted, Trust::Imported] {
        let guard = FileGuard::new(&root, trust, in_memory());
        let refusal = refused(guard.read("big.png"));
        assert_eq!(
            refusal.reason,
            SecurityReason::TooLarge {
                size: 64 * MIB + 1,
                cap: 64 * MIB
            },
            "{trust:?}: a texture's strict cap"
        );
        assert_eq!(refusal.layer, TrustLayer::File);
        assert_eq!(refusal.trust, trust);

        let edge = guard.read("scripts/edge.erg").unwrap_or_else(|error| {
            panic!("{trust:?}: a text exactly at its cap reads: {error:?}")
        });
        assert_eq!(edge.len() as u64, 16 * MIB);
    }

    let project = FileGuard::new(&root, Trust::Project, in_memory());
    assert_eq!(
        refused(project.read("huge.png")).reason,
        SecurityReason::TooLarge {
            size: 256 * MIB + 1,
            cap: 256 * MIB
        },
        "a texture's project cap"
    );
    assert_eq!(
        refused(project.read("scripts/long.erg")).reason,
        SecurityReason::TooLarge {
            size: 64 * MIB + 1,
            cap: 64 * MIB
        },
        "a text's project cap"
    );
}

/// **A downloaded file is read as `Imported`.**
///
/// A texture between the strict cap and the project's, carrying Windows'
/// download mark (`Zone.Identifier`), is refused under `Project` trust, at the
/// strict cap; the same file without the mark reads.
#[cfg(windows)]
#[test]
fn a_downloaded_file_is_read_as_imported() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let size = 64 * MIB + 1;
    let downloaded = root.join("downloaded.png");
    sized(&downloaded, size);
    sized(&root.join("made_here.png"), size);
    let mark = format!("{}:Zone.Identifier", downloaded.display());
    if fs::write(&mark, "[ZoneTransfer]\r\nZoneId=3\r\n").is_err() {
        log::warn!("this volume keeps no download mark: the mark is not checked");
        return;
    }

    let guard = FileGuard::new(&root, Trust::Project, in_memory());

    let refusal = refused(guard.read("downloaded.png"));
    assert_eq!(
        refusal.reason,
        SecurityReason::TooLarge {
            size,
            cap: 64 * MIB
        },
        "a downloaded texture has the strict cap"
    );
    assert_eq!(refusal.trust, Trust::Imported);
    let plain = guard
        .read("made_here.png")
        .expect("a file without the mark is read at the project's trust");
    assert_eq!(plain.len() as u64, size);
}

/// A guard resolves its root once, but a root made after the guard is read
/// once it exists.
#[test]
fn a_guard_made_before_its_root_exists_reads_once_it_is_made() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let guard = FileGuard::new(&root, Trust::Project, Refusals::detached());
    assert!(matches!(guard.read("a.txt"), Err(ReadError::Io(_))));

    fs::create_dir_all(&root).expect("the root is made");
    fs::write(root.join("a.txt"), b"made").expect("the file is made");
    assert_eq!(guard.read("a.txt").expect("it reads now"), b"made");
    assert_eq!(guard.read("a.txt").expect("and again"), b"made");
}

/// **A container's external file is read under its container's cap**, not
/// as a file of its own: a glTF's `.bin` over the strict cap of a file of no
/// asset reads as the glTF's.
#[test]
fn an_external_file_is_read_under_its_containers_cap() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let bin = root.join("models").join("a.bin");
    sized(&bin, 16 * MIB + 1);
    let guard = FileGuard::new(&root, Trust::Restricted, in_memory());

    assert_eq!(
        refused(guard.read("models/a.bin")).reason,
        SecurityReason::TooLarge {
            size: 16 * MIB + 1,
            cap: 16 * MIB
        },
        "read as a file of its own, it has the cap of no asset"
    );
    let read = guard
        .read_as("models/a.bin", FileKind::Gltf)
        .expect("read as its glTF's, it is under the glTF's cap");
    assert_eq!(read.len() as u64, 16 * MIB + 1);
}
