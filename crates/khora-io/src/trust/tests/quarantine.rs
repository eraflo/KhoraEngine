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

//! A refusal reported and quarantined, released, and handed over between runs.

use super::*;

/// A refusal of the file `name` under `dir/assets`, as a texture decoder
/// refuses one.
fn wrong_type_at(dir: &Path, name: &str) -> SecurityRefusal {
    SecurityRefusal {
        path: dir.join("assets").join(name),
        trust: Trust::Project,
        layer: TrustLayer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    }
}

/// **A refused file is quarantined and reported.**
///
/// - a refusal with a key is quarantined, held by that key only, reported to
///   the problems under `Security` (an error located at the file, with what
///   to do), and written to the store, where new refusals over the same
///   store find it — a stamp key as well as a content key;
/// - an `Escape` is reported but never stored, whatever key it comes with;
/// - a release lets the file be read again, here and in the store, and
///   clears its problem.
#[test]
fn a_refused_file_is_quarantined_and_reported() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let problems = Arc::new(Problems::default());
    let refusals = Refusals::new(Some(Arc::clone(&problems)), Some(store.clone()));

    let liar = dir.path().join("assets").join("liar.png");
    let wrong_type = SecurityRefusal {
        path: liar.clone(),
        trust: Trust::Project,
        layer: TrustLayer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    };
    let content = QuarantineKey::Content([7; 32]);
    refusals.refuse(wrong_type.clone(), Some(content));

    assert_eq!(refusals.quarantined(), vec![wrong_type.clone()]);
    assert!(refusals.is_quarantined(&liar, &content));
    assert!(
        !refusals.is_quarantined(&liar, &QuarantineKey::Content([8; 32])),
        "another content of the file is not quarantined"
    );
    let reported = security(&problems);
    assert_eq!(reported.len(), 1, "{reported:?}");
    assert_eq!(reported[0].severity, ProblemSeverity::Error);
    assert_eq!(reported[0].module, Some(liar.display().to_string()));
    assert!(!reported[0].message.is_empty());
    assert!(
        reported[0]
            .note
            .as_deref()
            .is_some_and(|note| !note.is_empty()),
        "the problem says what to do"
    );
    assert!(store.exists(), "the quarantine is written to its store");

    let huge = dir.path().join("assets").join("huge.png");
    let stamp = QuarantineKey::Stamp {
        len: 300 * MIB,
        modified: std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
    };
    refusals.refuse(
        SecurityRefusal {
            path: huge.clone(),
            trust: Trust::Project,
            layer: TrustLayer::File,
            reason: SecurityReason::TooLarge {
                size: 300 * MIB,
                cap: 256 * MIB,
            },
        },
        Some(stamp),
    );

    let escape = SecurityRefusal {
        path: PathBuf::from("assets/../../secret.png"),
        trust: Trust::Project,
        layer: TrustLayer::File,
        reason: SecurityReason::Escape,
    };
    refusals.refuse(escape.clone(), Some(QuarantineKey::Content([9; 32])));
    assert!(
        !refusals.quarantined().contains(&escape),
        "an escape is never stored"
    );
    assert!(
        security(&problems)
            .iter()
            .any(|problem| problem.module == Some(escape.path.display().to_string())),
        "an escape is reported"
    );

    let reread = Refusals::new(None, Some(store.clone()));
    assert!(
        reread.is_quarantined(&liar, &content),
        "read back from the store"
    );
    assert!(
        reread.is_quarantined(&huge, &stamp),
        "a stamp read back too"
    );
    assert!(!reread.quarantined().contains(&escape));

    refusals.release(&liar);
    assert!(!refusals.is_quarantined(&liar, &content));
    assert!(!refusals.quarantined().contains(&wrong_type));
    assert!(
        !security(&problems)
            .iter()
            .any(|problem| problem.module == Some(liar.display().to_string())),
        "a released file's problem is cleared"
    );
    let after_release = Refusals::new(None, Some(store));
    assert!(
        !after_release.is_quarantined(&liar, &content),
        "the release is written to the store"
    );
    assert!(after_release.is_quarantined(&huge, &stamp));
}

/// **A quarantined file is read again once it changes.**
///
/// Its content quarantined by its BLAKE3 hash is refused `Quarantined` and
/// reported; new bytes read; the old bytes back are refused again.
#[test]
fn a_quarantined_file_is_read_again_once_it_changes() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let refusals = reported_to(&problems);
    let file = at.root.join("textures").join("inside.png");
    refusals.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: TrustLayer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let guard = FileGuard::new(&at.root, Trust::Project, Arc::clone(&refusals));

    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined
    );

    fs::write(&file, b"new bytes").expect("writes");
    assert_eq!(
        guard
            .read("textures/inside.png")
            .expect("a quarantined file whose content changed reads"),
        b"new bytes"
    );

    fs::write(&file, b"inside bytes").expect("writes");
    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined,
        "the content quarantined, back"
    );
}

/// **Two runs sharing one store keep each other's quarantine.**
///
/// Two editors open at once (or the editor and a sandbox, both a developer's)
/// read the same `~/.khora/quarantine.ron` at boot, then each quarantines a
/// file of its own. The store ends holding both: a refusal written by one is
/// not erased by the other's next write.
#[test]
fn two_runs_sharing_a_store_keep_each_others_quarantine() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let first = Refusals::new(None, Some(store.clone()));
    let second = Refusals::new(None, Some(store.clone()));
    let refusal_of = |name: &str| SecurityRefusal {
        path: dir.path().join("assets").join(name),
        trust: Trust::Project,
        layer: TrustLayer::Decoder,
        reason: SecurityReason::WrongType {
            expected: "texture".to_owned(),
        },
    };

    first.refuse(refusal_of("one.png"), Some(QuarantineKey::Content([1; 32])));
    second.refuse(refusal_of("two.png"), Some(QuarantineKey::Content([2; 32])));

    let next_run = Refusals::new(None, Some(store));
    let mut quarantined: Vec<PathBuf> = (next_run.quarantined().into_iter())
        .map(|refusal| refusal.path)
        .collect();
    quarantined.sort();
    assert_eq!(
        quarantined,
        vec![
            dir.path().join("assets").join("one.png"),
            dir.path().join("assets").join("two.png"),
        ],
        "the first run's quarantine survives the second run's write"
    );
}

/// **A file refused for its size stops being reported once it fits.**
///
/// A script over its strict cap is refused `TooLarge` and quarantined by its
/// stamp; made smaller, as the remedy asks, it reads — and its refusal leaves
/// the problems and the store, rather than being reported at every boot for a
/// file that now loads.
#[test]
fn a_file_refused_for_its_size_is_no_longer_reported_once_it_fits() {
    let dir = TempDir::new().expect("a temp dir");
    let root = dir.path().join("assets");
    let store = dir.path().join("khora").join("quarantine.ron");
    let file = root.join("scripts").join("long.erg");
    sized(&file, 16 * MIB + 1);
    let problems = Arc::new(Problems::default());
    let refusals = Arc::new(Refusals::new(
        Some(Arc::clone(&problems)),
        Some(store.clone()),
    ));
    let guard = FileGuard::new(&root, Trust::Restricted, Arc::clone(&refusals));

    assert!(matches!(
        refused(guard.read("scripts/long.erg")).reason,
        SecurityReason::TooLarge { .. }
    ));
    assert!(!security(&problems).is_empty(), "the refusal is reported");

    fs::write(&file, "behavior Guard {}\n").expect("the script is made smaller");
    assert_eq!(
        guard
            .read("scripts/long.erg")
            .expect("the script, made smaller, reads"),
        b"behavior Guard {}\n"
    );

    let named = |problem: &Problem| {
        problem
            .module
            .as_deref()
            .is_some_and(|module| module.ends_with("long.erg"))
    };
    assert!(
        !security(&problems).iter().any(named),
        "a file that now reads is no longer reported refused: {:?}",
        security(&problems)
    );
    assert!(
        Refusals::new(None, Some(store)).quarantined().is_empty(),
        "nor kept in the store, to be reported again at the next boot"
    );
}

/// **A quarantined file is refused through any spelling of its root.**
///
/// The quarantine names a file, not the way its root was typed. The same
/// project opened as `<dir>/assets` in one run and `<dir>/elsewhere/../assets`
/// in the next reads the same file, whose quarantined content is refused.
#[test]
fn a_quarantined_file_is_refused_through_another_spelling_of_its_root() {
    let at = layout();
    let refusals = in_memory();
    refusals.refuse(
        SecurityRefusal {
            path: at.root.join("textures").join("inside.png"),
            trust: Trust::Project,
            layer: TrustLayer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let respelled = at.elsewhere.join("..").join("assets");
    let guard = FileGuard::new(&respelled, Trust::Project, Arc::clone(&refusals));

    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined,
        "the quarantined content, read through {}",
        respelled.display()
    );
}

/// **A release by one run sticks though another run sharing the store held the
/// file.**
///
/// Two runs boot over one store holding a quarantined file; the first releases
/// it; the second then quarantines another file. The store must not hold the
/// released file again — the second run's write may keep what others
/// quarantined, not bring back what they released.
#[test]
fn a_release_sticks_when_another_run_sharing_the_store_writes() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let released = wrong_type_at(dir.path(), "released.png");
    let key = QuarantineKey::Content([1; 32]);
    Refusals::new(None, Some(store.clone())).refuse(released.clone(), Some(key));

    let first = Refusals::new(None, Some(store.clone()));
    let second = Refusals::new(None, Some(store.clone()));
    first.release(&released.path);
    second.refuse(
        wrong_type_at(dir.path(), "other.png"),
        Some(QuarantineKey::Content([2; 32])),
    );

    assert!(
        !Refusals::new(None, Some(store)).is_quarantined(&released.path, &key),
        "the file the first run released is back in the store after the second run's write"
    );
}

/// **A file quarantined by another run after this one released it stays in the
/// store.**
///
/// The first run releases a file; the second, sharing the store, then refuses
/// that file's new content; the first's next write, for another file, must keep
/// the second's quarantine — a release covers what was released, not what
/// another run refused since.
#[test]
fn a_file_quarantined_by_another_run_after_a_release_here_stays_in_the_store() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    let first = Refusals::new(None, Some(store.clone()));
    let second = Refusals::new(None, Some(store.clone()));
    let file = wrong_type_at(dir.path(), "liar.png");

    first.refuse(file.clone(), Some(QuarantineKey::Content([1; 32])));
    first.release(&file.path);
    let refused_since = QuarantineKey::Content([2; 32]);
    second.refuse(file.clone(), Some(refused_since));
    first.refuse(
        wrong_type_at(dir.path(), "other.png"),
        Some(QuarantineKey::Content([3; 32])),
    );

    assert!(
        Refusals::new(None, Some(store)).is_quarantined(&file.path, &refused_since),
        "the second run's quarantine of the file is dropped by the first run's write"
    );
}

/// **A file read with another content stops being reported.**
///
/// Its content quarantined, the file reads once it holds other bytes; it leaves
/// the problems and `quarantined()`, though its old content is still refused
/// and reported again when it comes back.
#[test]
fn a_file_read_with_another_content_is_no_longer_reported() {
    let at = layout();
    let problems = Arc::new(Problems::default());
    let refusals = reported_to(&problems);
    let file = at.root.join("textures").join("inside.png");
    refusals.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: TrustLayer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let guard = FileGuard::new(&at.root, Trust::Project, Arc::clone(&refusals));

    fs::write(&file, b"new bytes").expect("writes");
    guard
        .read("textures/inside.png")
        .expect("the new content reads");
    assert!(refusals.quarantined().is_empty());
    assert!(security(&problems).is_empty(), "{:?}", security(&problems));

    fs::write(&file, b"inside bytes").expect("writes");
    assert_eq!(
        refused(guard.read("textures/inside.png")).reason,
        SecurityReason::Quarantined
    );
    assert_eq!(refusals.quarantined().len(), 1);
    assert_eq!(security(&problems).len(), 1, "{:?}", security(&problems));
}

/// **Refusals handed over keep a file that reads again unreported.**
///
/// A guard made before the run's refusals were known quarantined a file's
/// content, then read the file with other content — it stops being reported.
/// Handed over to the run's refusals, the file must stay unreported there,
/// while its old content stays refused.
#[test]
fn refusals_handed_over_keep_a_file_that_reads_again_unreported() {
    let at = layout();
    let former = in_memory();
    let file = at.root.join("textures").join("inside.png");
    former.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: TrustLayer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content(
            *blake3::hash(b"inside bytes").as_bytes(),
        )),
    );
    let guard = FileGuard::new(&at.root, Trust::Project, Arc::clone(&former));
    fs::write(&file, b"new bytes").expect("writes");
    guard
        .read("textures/inside.png")
        .expect("the new content reads");
    assert!(former.quarantined().is_empty());

    let problems = Arc::new(Problems::default());
    let run = reported_to(&problems);
    former.hand_over_to(Arc::clone(&run));

    assert!(
        run.quarantined().is_empty(),
        "a file that reads with another content is quarantined again by the hand-over: {:?}",
        run.quarantined()
    );
    assert!(
        security(&problems).is_empty(),
        "and reported again: {:?}",
        security(&problems)
    );
}

/// **A store written before entries named their real path reads.**
///
/// An entry with only its refusal and key is quarantined by the refusal's path,
/// and survives the next write.
#[test]
fn a_store_from_before_real_paths_reads() {
    let dir = TempDir::new().expect("a temp dir");
    let store = dir.path().join("khora").join("quarantine.ron");
    fs::create_dir_all(store.parent().expect("a folder")).expect("the folder is made");
    let old = wrong_type_at(dir.path(), "old.png");
    let bytes = vec!["7"; 32].join(", ");
    fs::write(
        &store,
        format!(
            "[(refusal: (path: {:?}, trust: Project, layer: Decoder, \
             reason: WrongType(expected: \"texture\")), key: Content(({bytes}))),]",
            old.path.display().to_string()
        ),
    )
    .expect("writes");
    let key = QuarantineKey::Content([7; 32]);

    let refusals = Refusals::new(None, Some(store.clone()));
    assert!(refusals.is_quarantined(&old.path, &key));
    assert_eq!(refusals.quarantined(), vec![old.clone()]);

    refusals.refuse(
        wrong_type_at(dir.path(), "new.png"),
        Some(QuarantineKey::Content([8; 32])),
    );
    assert!(Refusals::new(None, Some(store)).is_quarantined(&old.path, &key));
}

/// The reads a run makes report their refusals to the run's [`Refusals`].
///
/// What the project's script compile and the glTF reads of an asset service
/// refuse reaches the run's problems under `Security`.
mod refusals_reach_the_run {
    use std::fs;
    use std::sync::Arc;

    use khora_core::problems::Problems;

    use super::{layout, link_dir, reported_to, security, sized, MIB};
    use crate::trust::{FileGuard, Refusals, Trust};

    /// Whether a problem of the `Security` source names a file ending with
    /// `name`.
    fn reports(problems: &Problems, name: &str) -> bool {
        security(problems).iter().any(|problem| {
            problem
                .module
                .as_deref()
                .is_some_and(|module| module.ends_with(name))
        })
    }

    /// **The project's script compile reports its refusals to the run.**
    ///
    /// A module over its cap, compiled by a `ProjectCompiler` made from a
    /// runtime holding the run's refusals and problems, is refused by the guard
    /// — and that refusal is reported under `Security`, as every other read of
    /// the run is.
    #[test]
    fn the_projects_script_compile_reports_its_refusals_to_the_run() {
        use crate::script::project_compiler::ProjectCompiler;

        let at = layout();
        let scripts = at.root.join("scripts");
        fs::create_dir_all(&scripts).expect("the folder is made");
        fs::write(scripts.join("guard.erg"), "behavior Guard {}\n").expect("writes");
        sized(&scripts.join("long.erg"), 64 * MIB + 1);

        let problems = Arc::new(Problems::default());
        let mut runtime = khora_core::Runtime::default();
        runtime.resources.insert(Arc::clone(&problems));
        runtime.resources.insert(reported_to(&problems));

        let _ = ProjectCompiler::from_runtime(&runtime).compile_all(&scripts);

        assert!(
            reports(&problems, "long.erg"),
            "the module refused for its size is reported: {:?}",
            security(&problems)
        );
    }

    /// **A service given the run's refusals after its decoders reports its glTF
    /// reads to them.**
    ///
    /// As a packed game's launcher does — the service built and its default
    /// decoders registered, then the run's refusals set by the bootstrap — a
    /// glTF buffer reached through a link leaving the assets root is refused,
    /// and reported to the run, not to refusals the service made for itself
    /// before.
    #[test]
    fn a_gltf_read_refused_after_the_service_takes_the_runs_refusals_is_reported_to_them() {
        use crate::asset::{AssetService, FileLoader, IndexBuilder};
        use khora_core::asset::AssetUUID;
        use khora_core::renderer::api::gpu_scene::Mesh;

        let at = layout();
        let models = at.root.join("models");
        fs::create_dir_all(&models).expect("the folder is made");
        fs::write(at.elsewhere.join("secret.bin"), [0u8; 36]).expect("writes");
        let gltf = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [ { "uri": "linked/secret.bin", "byteLength": 36 } ],
            "bufferViews": [ { "buffer": 0, "byteOffset": 0, "byteLength": 36 } ],
            "accessors": [ {
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0],
            } ],
            "meshes": [ { "primitives": [ { "attributes": { "POSITION": 0 } } ] } ],
            "nodes": [ { "mesh": 0 } ],
            "scenes": [ { "nodes": [0] } ],
            "scene": 0,
        });
        fs::write(
            models.join("scene.gltf"),
            serde_json::to_vec(&gltf).expect("the glTF serialises"),
        )
        .expect("writes");
        assert!(
            link_dir(&at.elsewhere, &models.join("linked")),
            "this system makes a link (a junction on Windows)"
        );

        let problems = Arc::new(Problems::default());
        let run: Arc<Refusals> = reported_to(&problems);
        let index = IndexBuilder::new(crate::trust::FileGuard::new(
            &at.root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .expect("the folder indexes");
        let loader =
            FileLoader::guarded(FileGuard::new(&at.root, Trust::Project, Arc::clone(&run)));
        let mut service = AssetService::new(
            &index,
            Box::new(loader),
            Arc::new(khora_telemetry::MetricsRegistry::new()),
            None,
        )
        .expect("a service over the index");
        service.register_default_decoders(&at.root);
        service.set_refusals(Arc::clone(&run));

        let whole = AssetUUID::new_v5("models/scene.gltf");
        let _ = service.load::<Mesh>(&whole);
        let _ = service.load::<Mesh>(&AssetUUID::sub_asset(whole, "mesh0/prim0"));

        assert!(
            reports(&problems, "secret.bin"),
            "the glTF buffer leaving the root is reported to the run: {:?}",
            security(&problems)
        );
    }
}

/// **A hand-over back to refusals handed over to is ignored.**
///
/// After `a` hands over to `b`, `b` handing over to `a` would forward every
/// call between them forever; it is refused, and both still answer.
#[test]
fn a_hand_over_back_to_refusals_handed_over_to_is_ignored() {
    let a = in_memory();
    let b = in_memory();
    a.hand_over_to(Arc::clone(&b));
    b.hand_over_to(Arc::clone(&a));

    let file = PathBuf::from("assets/liar.png");
    a.refuse(
        SecurityRefusal {
            path: file.clone(),
            trust: Trust::Project,
            layer: TrustLayer::Decoder,
            reason: SecurityReason::WrongType {
                expected: "texture".to_owned(),
            },
        },
        Some(QuarantineKey::Content([3; 32])),
    );
    assert_eq!(a.quarantined().len(), 1, "a forwards to b");
    assert_eq!(b.quarantined().len(), 1, "b holds what a forwarded");
}
