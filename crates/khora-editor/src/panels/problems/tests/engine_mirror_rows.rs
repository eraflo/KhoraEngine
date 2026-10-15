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

//! **A problem located in the engine's component mirror opens no file**:
//! `engine/components.erg` is served by the engine, never written under a
//! project's `assets/scripts/`, so a row there must not send the system to
//! open a path that does not exist.

use khora_sdk::khora_core::script::{ProblemSeverity, ScriptProblem};

use crate::panels::problems::view::{row_action, ProblemAction};

/// The module the engine serves its component mirror as — what a script
/// writes after `import`.
const ENGINE_MIRROR: &str = "engine/components.erg";

#[test]
fn a_problem_in_the_engine_mirror_does_not_open_a_project_file() {
    let problem = ScriptProblem {
        severity: ProblemSeverity::Error,
        module: Some(ENGINE_MIRROR.to_owned()),
        line: 3,
        column: 8,
        message: "`Transform` is declared in both `helper.erg` and `engine/components.erg`"
            .to_owned(),
        note: None,
        entity: None,
        behavior: None,
    };

    assert_ne!(
        row_action(&problem),
        ProblemAction::OpenModule(ENGINE_MIRROR.to_owned()),
        "the mirror is no file under the project's scripts"
    );
}
