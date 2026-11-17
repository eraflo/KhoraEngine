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

//! What the project cards show of each project's folder, read from disk once and kept a while.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use khora_sdk::trust::TrustStore;

use super::CardFact;

/// How long the facts read are kept: the editor, another program, may change them meanwhile.
const KEPT_FOR: Duration = Duration::from_secs(1);

/// The facts of the cards painted, by project path: each read at most once a second.
///
/// A card painted every frame would otherwise resolve its folder and stat
/// its files every frame.
#[derive(Default)]
pub struct CardFacts {
    facts: HashMap<String, CardFact>,
    /// When the facts kept were first read; `None` when none are.
    read_at: Option<Instant>,
}

impl CardFacts {
    /// Returns the facts of the project at `path`: its trust read through `store` and its
    /// folder from disk, unless kept.
    pub fn of(&mut self, path: &str, store: &TrustStore) -> CardFact {
        let now = Instant::now();
        if self
            .read_at
            .is_none_or(|at| now.duration_since(at) >= KEPT_FOR)
        {
            self.facts.clear();
            self.read_at = Some(now);
        }
        *self.facts.entry(path.to_owned()).or_insert_with(|| {
            let root = Path::new(path);
            CardFact {
                trust: store.decision(root),
                native_code: crate::services::project::has_native_code(root),
            }
        })
    }

    /// Forgets every fact kept: the hub changed one, and the next paint reads them again.
    pub fn forget(&mut self) {
        self.facts.clear();
        self.read_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::trust::ProjectTrust;
    use std::path::PathBuf;

    /// A project folder under the temp folder, removed when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "khora-hub-card-facts-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_nanos())
            ));
            std::fs::create_dir_all(&root).expect("the folder");
            Folder(root)
        }

        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_card_shows_its_projects_decision_and_native_code() {
        let project = Folder::new("read");
        std::fs::write(project.0.join("Cargo.toml"), b"[package]").expect("a manifest");
        let store = TrustStore::new(None);
        store.decide(&project.0, ProjectTrust::Restricted);
        let mut facts = CardFacts::default();
        assert_eq!(
            facts.of(&project.path(), &store),
            CardFact {
                trust: Some(ProjectTrust::Restricted),
                native_code: true,
            }
        );
    }

    #[test]
    fn a_fact_read_is_kept_until_forgotten() {
        let project = Folder::new("kept");
        let store = TrustStore::new(None);
        let mut facts = CardFacts::default();
        assert_eq!(facts.of(&project.path(), &store).trust, None);

        store.decide(&project.0, ProjectTrust::Trusted);
        std::fs::write(project.0.join("Cargo.toml"), b"[package]").expect("a manifest");
        assert_eq!(
            facts.of(&project.path(), &store),
            CardFact {
                trust: None,
                native_code: false,
            },
            "kept: not read again within the second"
        );

        facts.forget();
        assert_eq!(
            facts.of(&project.path(), &store),
            CardFact {
                trust: Some(ProjectTrust::Trusted),
                native_code: true,
            },
            "read again once forgotten"
        );
    }

    #[test]
    fn a_fact_kept_past_its_time_is_read_again() {
        let project = Folder::new("stale");
        let store = TrustStore::new(None);
        let mut facts = CardFacts::default();
        facts.of(&project.path(), &store);
        store.decide(&project.0, ProjectTrust::Restricted);
        facts.read_at = Instant::now().checked_sub(KEPT_FOR);
        assert_eq!(
            facts.of(&project.path(), &store).trust,
            Some(ProjectTrust::Restricted)
        );
    }
}
