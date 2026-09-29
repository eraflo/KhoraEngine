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

/// Read the current git branch name from a project's `.git/HEAD`.
///
/// Returns `Some(branch_name)` if the project is a git repository on a
/// regular branch, `None` if the project isn't tracked or HEAD is detached.
/// Cheap (single fs read of a small file) — safe to call once at startup.
pub fn read_git_branch(project_root: &std::path::Path) -> Option<String> {
    let head_path = project_root.join(".git").join("HEAD");
    let head = std::fs::read_to_string(head_path).ok()?;
    let line = head.lines().next()?;
    // Format: `ref: refs/heads/<branch>` or a raw SHA when detached.
    line.strip_prefix("ref: refs/heads/").map(|s| s.to_owned())
}
