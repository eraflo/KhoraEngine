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

//! A refusal the hub met reading a project's file.

use std::path::PathBuf;

/// A refusal met reading one of a project's files, with the project it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRefusal {
    /// The project's folder.
    pub project: PathBuf,
    /// The project's name, as the hub shows it.
    pub name: String,
    /// What was refused, and why.
    pub message: String,
}

impl ProjectRefusal {
    /// Returns the refusal as the hub shows it: the project's name, then what was refused.
    pub fn shown(&self) -> String {
        format!("'{}': {}", self.name, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::ProjectRefusal;

    /// **A refusal is shown with the project it belongs to.**
    #[test]
    fn a_refusal_is_shown_with_its_project() {
        let refusal = ProjectRefusal {
            project: "/projects/ember".into(),
            name: "ember".to_owned(),
            message: "project.json is not read: refused".to_owned(),
        };

        assert_eq!(
            refusal.shown(),
            "'ember': project.json is not read: refused"
        );
    }
}
