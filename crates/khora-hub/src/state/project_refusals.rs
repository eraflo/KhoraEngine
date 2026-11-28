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

//! The refusals the hub met reading the projects' files, each kept until dismissed.

use std::path::Path;

use super::ProjectRefusal;

/// The refusals the hub met reading the projects' files, each kept until the user dismisses it.
///
/// Apart from the banner: a launch's banner shows the refusals of the
/// project it launches, and another banner taking its place leaves them listed.
#[derive(Debug, Default)]
pub struct ProjectRefusals {
    /// In the order they were met; one per project and message.
    listed: Vec<ProjectRefusal>,
}

impl ProjectRefusals {
    /// Keeps `refusal` until it is dismissed; one already listed, same project and message, is kept once.
    pub fn record(&mut self, refusal: ProjectRefusal) {
        let listed = self
            .listed
            .iter()
            .any(|kept| kept.project == refusal.project && kept.message == refusal.message);
        if !listed {
            self.listed.push(refusal);
        }
    }

    /// Returns the refusals of the project at `project`, in the order they were met.
    pub fn of<'a>(&'a self, project: &'a Path) -> impl Iterator<Item = &'a ProjectRefusal> + 'a {
        self.listed
            .iter()
            .filter(move |refusal| refusal.project == project)
    }

    /// Dismisses every refusal of the project at `project`.
    pub fn dismiss(&mut self, project: &Path) {
        self.listed.retain(|refusal| refusal.project != project);
    }

    /// Dismisses `refusal` alone.
    pub fn dismiss_one(&mut self, refusal: &ProjectRefusal) {
        self.listed.retain(|kept| kept != refusal);
    }

    /// Returns every refusal kept, in the order they were met.
    pub fn iter(&self) -> std::slice::Iter<'_, ProjectRefusal> {
        self.listed.iter()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{ProjectRefusal, ProjectRefusals};

    /// A refusal of the project at `project`, saying `message`.
    fn refusal(project: &str, message: &str) -> ProjectRefusal {
        ProjectRefusal {
            project: project.into(),
            name: project.trim_start_matches('/').to_owned(),
            message: message.to_owned(),
        }
    }

    /// **Each refusal is kept, once, with its project, until its project's are dismissed.**
    #[test]
    fn each_refusal_is_kept_once_with_its_project_until_dismissed() {
        let mut refusals = ProjectRefusals::default();
        refusals.record(refusal("/ember", "project.json is not read"));
        refusals.record(refusal("/lantern", "project.json is not read"));
        refusals.record(refusal("/ember", "project.json is not read"));

        assert_eq!(
            refusals.iter().count(),
            2,
            "a refusal met again is kept once"
        );
        assert_eq!(refusals.of(Path::new("/ember")).count(), 1);

        refusals.dismiss(Path::new("/ember"));

        assert_eq!(refusals.of(Path::new("/ember")).count(), 0);
        let kept: Vec<&str> = refusals.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(kept, ["lantern"], "another project's refusal stays");
    }

    /// **A refusal dismissed alone leaves the others of its project.**
    #[test]
    fn a_refusal_dismissed_alone_leaves_the_others_of_its_project() {
        let mut refusals = ProjectRefusals::default();
        refusals.record(refusal("/ember", "project.json is not read"));
        refusals.record(refusal("/ember", "icon.png is not read"));

        refusals.dismiss_one(&refusal("/ember", "project.json is not read"));

        let kept: Vec<&str> = refusals.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(kept, ["icon.png is not read"]);
    }
}
