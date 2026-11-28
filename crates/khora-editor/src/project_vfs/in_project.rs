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

//! Whether a path picked by the user is one of the project's assets.

/// Where a path picked by the user lies, for the project open: the answer of [`ProjectVfs::in_project`](super::ProjectVfs::in_project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InProject {
    /// Outside the project's `assets/`: a file of no project, read or
    /// written where it is.
    Outside,
    /// The file at this forward-slash path under `assets/`.
    Inside(String),
    /// Under the project's `assets/`, but no path a project keeps, saying
    /// why: nothing is read or written there.
    Refused(String),
}
