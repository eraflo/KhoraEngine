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

//! The writes a reindex holds back, and the reindex a write files.

use super::*;

/// **Without a hub, a write reindexes at once.**
///
/// A project opened with no engine has no index hub: `reindex` rebuilds
/// the index on the frame, and an action asked for after it runs now.
#[test]
fn without_a_hub_writes_reindex_at_once() -> Result<()> {
    const PLAYER: &str = "scripts/Player.erg";
    let project = TempProject::new("no-hub-reindex")?;
    let mut pvfs = project.open()?;
    pvfs.write_asset(Path::new(PLAYER), b"behavior Player { }")?;
    let player = pvfs.resolve_uuid(PLAYER);
    assert!(
        pvfs.assets().index().get_metadata(&player).is_none(),
        "the premise: a write alone does not index"
    );
    let before = pvfs.generation();

    pvfs.reindex()?;

    assert!(
        pvfs.assets().index().get_metadata(&player).is_some(),
        "the file is indexed when `reindex` returns"
    );
    assert!(pvfs.generation() > before, "the project's generation moved");
    assert_eq!(
        pvfs.after_index(reveal(PLAYER)),
        Some(reveal(PLAYER)),
        "nothing is in flight: the action runs now"
    );
    assert!(pvfs.land().is_none(), "nothing was filed, nothing lands");
    Ok(())
}

/// **A restricted project's reindex reads under the strict caps**.
///
/// A material over them is refused, and the refusal reaches the run's refusals.
#[test]
fn a_restricted_projects_reindex_refuses_a_material_over_the_strict_cap() -> Result<()> {
    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, SecurityReason};

    let project = TempProject::new("restricted-reindex")?;
    let mut pvfs = project.open()?;
    let materials = project.0.join("assets").join("materials");
    std::fs::create_dir_all(&materials)?;
    std::fs::File::create(materials.join("big.kmat"))?.set_len(16 * (1 << 20) + 1)?;
    let refusals = Arc::new(Refusals::new(None, None));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
    pvfs.reads = ProjectReads::new(Arc::clone(&refusals), trust);
    assert!(Arc::ptr_eq(&pvfs.reads().refusals(), &refusals));

    pvfs.rebuild_index()?;

    let refused: Vec<_> = refusals
        .quarantined()
        .into_iter()
        .filter(|refusal| refusal.path.ends_with("materials/big.kmat"))
        .collect();
    assert_eq!(refused.len(), 1, "the material is refused and reported");
    assert!(
        matches!(refused[0].reason, SecurityReason::TooLarge { .. }),
        "refused for its size: {:?}",
        refused[0].reason
    );
    Ok(())
}

/// **With a hub and nothing in flight, an action runs now.** No write
/// waits to be indexed: `after_index` hands the action back.
#[test]
fn with_a_hub_and_nothing_in_flight_an_action_runs_now() {
    use crate::tests::gltf_fixture::GltfProject;
    use crate::tests::project_index::IndexingEngine;

    let engine = IndexingEngine::new();
    let project = GltfProject::with("hub-nothing-in-flight", &[]);
    engine.serve(&project.vfs);
    let mut pvfs = project.vfs.lock().expect("the project");

    assert_eq!(
        pvfs.after_index(reveal("scripts/Player.erg")),
        Some(reveal("scripts/Player.erg"))
    );
    assert!(pvfs.land().is_none(), "nothing was filed, nothing lands");
    assert_eq!(engine.filed(), 0, "no reindex was filed");
}

/// **A write during a reindex waits for the next.**
///
/// The first reindex's walk began before the second write, so its landing
/// runs only the first write's action. The second reindex is filed then,
/// one in flight at a time, and its landing runs the second's. Each
/// landing moves the project's generation.
#[test]
fn a_write_during_a_reindex_waits_for_the_next() {
    use crate::tests::gltf_fixture::GltfProject;
    use crate::tests::project_index::IndexingEngine;

    const FIRST: &str = "scripts/First.erg";
    const SECOND: &str = "scripts/Second.erg";
    let engine = IndexingEngine::new();
    let project = GltfProject::with("write-during-reindex", &[]);
    engine.serve(&project.vfs);
    let (first, second) = (project.uuid(FIRST), project.uuid(SECOND));
    let mut pvfs = project.vfs.lock().expect("the project");

    pvfs.create_asset(FIRST, b"behavior First { }")
        .expect("the first file is created");
    assert_eq!(
        pvfs.after_index(reveal(FIRST)),
        None,
        "the first action waits for the reindex in flight"
    );
    assert_eq!(engine.waiting(), 1, "the first write filed a reindex");
    // The first reindex walks the project now, before the second write.
    engine.build();

    pvfs.create_asset(SECOND, b"behavior Second { }")
        .expect("the second file is created");
    assert_eq!(
        pvfs.after_index(reveal(SECOND)),
        None,
        "the second action waits for the next reindex"
    );
    assert_eq!(
        engine.waiting(),
        0,
        "no second reindex while the first is in flight"
    );
    assert!(pvfs.land().is_none(), "the first is still in flight");

    engine.intake();
    let generation = pvfs.generation();
    let landed = pvfs.land().expect("the first reindex landed");
    assert_eq!(landed.outcome, Ok(()));
    assert_eq!(
        landed.actions,
        vec![reveal(FIRST)],
        "only the first write's action runs at the first landing"
    );
    assert!(
        pvfs.generation() > generation,
        "the landing moves the generation"
    );
    assert!(engine.indexes(&first), "the first file is indexed");
    assert!(
        !engine.indexes(&second),
        "the second file is not: the walk began before it"
    );
    assert_eq!(
        engine.waiting(),
        1,
        "the second reindex is filed at the first's landing"
    );
    assert!(pvfs.land().is_none(), "the second is in flight");

    engine.frame();
    let generation = pvfs.generation();
    let landed = pvfs.land().expect("the second reindex landed");
    assert_eq!(landed.outcome, Ok(()));
    assert_eq!(
        landed.actions,
        vec![reveal(SECOND)],
        "the second write's action runs at the second landing"
    );
    assert!(
        pvfs.generation() > generation,
        "the landing moves the generation"
    );
    assert!(engine.indexes(&second), "the second file is indexed");

    assert!(pvfs.land().is_none(), "nothing more lands");
    assert_eq!(engine.waiting(), 0, "nothing more is filed");
    assert_eq!(engine.filed(), 2, "two reindexes in all");
    assert_eq!(engine.most_waiting(), 1, "one reindex in flight at a time");
}
