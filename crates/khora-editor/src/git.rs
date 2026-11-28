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

//! Utility helpers for the editor application.

use khora_sdk::trust::{FileGuard, ProjectFile};

/// Returns the git branch checked out in the project `project` guards, read from its `.git/HEAD`.
///
/// `None` when the project is not a git repository or its HEAD is detached;
/// when the guard refuses the file, the refusal reported to its refusals.
pub(crate) fn read_git_branch(project: &FileGuard) -> Option<String> {
    let head = match project.read_project_file(ProjectFile::GitHead) {
        Ok(head) => head,
        Err(error) if error.is_missing() => return None,
        Err(error) => {
            log::warn!("the project's `.git/HEAD` is not read: {error}");
            return None;
        }
    };
    let head = String::from_utf8(head).ok()?;
    let line = head.lines().next()?;
    // Format: `ref: refs/heads/<branch>` or a raw SHA when detached.
    line.strip_prefix("ref: refs/heads/").map(|s| s.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use khora_sdk::trust::{FileGuard, Refusals, Trust};

    use super::read_git_branch;

    /// **The branch is read from `.git/HEAD` through the guard**; a HEAD over
    /// its cap names none, and is reported.
    #[test]
    fn the_branch_is_read_through_the_guard() {
        let root =
            std::env::temp_dir().join(format!("khora-editor-git-head-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).expect("the git folder");
        let head = root.join(".git").join("HEAD");
        std::fs::write(&head, "ref: refs/heads/main\n").expect("writes");
        let refusals = Arc::new(Refusals::new(None, None));
        let guard = FileGuard::new(&root, Trust::Project, Arc::clone(&refusals));

        let branch = read_git_branch(&guard);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&head)
            .and_then(|file| file.set_len((4 << 10) + 1))
            .expect("the HEAD grows past its cap");
        let oversized = read_git_branch(&guard);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(branch.as_deref(), Some("main"));
        assert_eq!(oversized, None);
        assert!(
            refusals
                .quarantined()
                .iter()
                .any(|refusal| refusal.path.ends_with("HEAD")),
            "{:?}",
            refusals.quarantined()
        );
    }
}
