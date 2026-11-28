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

//! A scene file outside the project's `assets/` is read through a guard: the
//! project root's, at the project's trust, or its folder's, as a downloaded file.

use std::sync::Arc;

use khora_sdk::khora_data::scene::loading::SceneSource;
use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, SecurityReason, Trust};

use crate::project_vfs::ProjectReads;

use super::super::scene_on_disk;

/// A folder under the temp folder, removed when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-scene-on-disk-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the folder");
        Folder(root)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Makes a scene file at `path` of `len` bytes.
fn scene_of(path: &std::path::Path, len: u64) {
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the scene's folder");
    std::fs::File::create(path)
        .and_then(|file| file.set_len(len))
        .expect("the scene is made");
}

/// Returns what fetching `source` gives.
fn fetched(source: &SceneSource) -> Result<Vec<u8>, String> {
    match source {
        SceneSource::Fetch(fetch) => fetch.fetch(),
        SceneSource::Bytes(bytes) => Ok(bytes.to_vec()),
    }
}

/// **A scene of a restricted project outside its `assets/` is read under the
/// strict cap**, and its refusal reported to the run's refusals.
#[test]
fn a_scene_outside_assets_is_read_at_the_projects_trust() {
    let project = Folder::new("restricted");
    let scene = project.0.join("scenes").join("big.kscene");
    scene_of(&scene, (16 << 20) + 1);
    let refusals = Arc::new(Refusals::new(None, None));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
    let reads = ProjectReads::new(Arc::clone(&refusals), trust);

    let (source, _) = scene_on_disk(&scene, &reads, Some(&project.0));

    let error = fetched(&source).expect_err("the scene is over the strict cap");
    assert!(error.contains("big.kscene"), "{error}");
    assert!(
        refusals.quarantined().iter().any(|refusal| {
            refusal.path.ends_with("big.kscene")
                && refusal.trust == Trust::Restricted
                && matches!(refusal.reason, SecurityReason::TooLarge { .. })
        }),
        "{:?}",
        refusals.quarantined()
    );
}

/// **A scene outside any project is read as a downloaded file**: under the
/// strict caps, whatever the project open is trusted, reported to the run's
/// refusals; one under them reads.
#[test]
fn a_scene_outside_the_project_is_read_as_a_downloaded_file() {
    let project = Folder::new("trusted");
    let elsewhere = Folder::new("elsewhere");
    let big = elsewhere.0.join("big.kscene");
    scene_of(&big, (16 << 20) + 1);
    let small = elsewhere.0.join("small.kscene");
    scene_of(&small, 3);
    let refusals = Arc::new(Refusals::new(None, None));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Trusted));
    let reads = ProjectReads::new(Arc::clone(&refusals), trust);

    let (refused, _) = scene_on_disk(&big, &reads, Some(&project.0));
    let (read, _) = scene_on_disk(&small, &reads, Some(&project.0));

    assert!(fetched(&refused).is_err());
    assert_eq!(fetched(&read).expect("the small scene reads").len(), 3);
    assert!(
        refusals.quarantined().iter().any(|refusal| {
            refusal.path.ends_with("big.kscene") && refusal.trust == Trust::Imported
        }),
        "{:?}",
        refusals.quarantined()
    );
}
