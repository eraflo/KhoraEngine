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

//! What the Problems tab and the `.erg` tiles show, computed from the
//! engine's `ScriptProblems` without drawing anything.

use std::path::{Path, PathBuf};

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::script::{ProblemSeverity, ScriptProblem};

/// One module's problems, as the tab lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProblemGroup {
    /// The module, `None` for the problems in no module.
    pub module: Option<String>,
    /// Its problems, in the order given.
    pub rows: Vec<ProblemRow>,
}

/// One problem, as the tab lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProblemRow {
    /// Error or warning.
    pub severity: ProblemSeverity,
    /// `line:col  message`, or the message alone when it has no position.
    pub text: String,
    /// Why, or what to do about it.
    pub note: Option<String>,
    /// The entity it happened on, named, for a running problem.
    pub entity: Option<String>,
    /// What clicking the row does.
    pub action: ProblemAction,
}

/// What clicking a problem's row does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProblemAction {
    /// Opens the module's file in the system's app.
    OpenModule(String),
    /// Selects the entity it happened on.
    SelectEntity(EntityId),
    /// Nothing to go to.
    Nothing,
}

/// The badge an `.erg` tile wears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TileBadge {
    /// The worst of what the module has.
    pub severity: ProblemSeverity,
    /// How many of that severity.
    pub count: usize,
}

/// `problems` grouped by module, each entity named by `name_of`: the modules
/// in name order, the problems in no module last, each group's rows in the
/// order given.
pub(crate) fn grouped(
    problems: &[ScriptProblem],
    name_of: &dyn Fn(EntityId) -> Option<String>,
) -> Vec<ProblemGroup> {
    let mut groups: Vec<ProblemGroup> = Vec::new();
    for problem in problems {
        let row = ProblemRow {
            severity: problem.severity,
            text: if problem.line == 0 {
                problem.message.clone()
            } else {
                format!("{}:{}  {}", problem.line, problem.column, problem.message)
            },
            note: problem.note.clone(),
            entity: problem.entity.map(|entity| {
                name_of(entity).unwrap_or_else(|| format!("Entity {}", entity.index))
            }),
            action: row_action(problem),
        };
        match groups
            .iter_mut()
            .find(|group| group.module == problem.module)
        {
            Some(group) => group.rows.push(row),
            None => groups.push(ProblemGroup {
                module: problem.module.clone(),
                rows: vec![row],
            }),
        }
    }
    // `None` sorts first as an `Option`; the problems in no module go last.
    groups.sort_by(|a, b| (a.module.is_none(), &a.module).cmp(&(b.module.is_none(), &b.module)));
    groups
}

/// What clicking `problem`'s row does: a running problem selects its entity,
/// any other in a module opens that module.
pub(crate) fn row_action(problem: &ScriptProblem) -> ProblemAction {
    match (problem.entity, &problem.module) {
        (Some(entity), _) => ProblemAction::SelectEntity(entity),
        // The engine's mirror is served, not a file of the project.
        (None, Some(module)) if module == khora_sdk::scripts::ENGINE_MODULE => {
            ProblemAction::Nothing
        }
        (None, Some(module)) => ProblemAction::OpenModule(module.clone()),
        (None, None) => ProblemAction::Nothing,
    }
}

/// The tab's title, with its counts of errors and warnings.
pub(crate) fn tab_title(problems: &[ScriptProblem]) -> String {
    let errors = problems
        .iter()
        .filter(|problem| problem.severity == ProblemSeverity::Error)
        .count();
    let warnings = problems.len() - errors;
    let count =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match (errors, warnings) {
        (0, 0) => "Problems".to_owned(),
        (errors, 0) => format!("Problems · {}", count(errors, "error", "errors")),
        (0, warnings) => format!("Problems · {}", count(warnings, "warning", "warnings")),
        (errors, warnings) => format!(
            "Problems · {} · {}",
            count(errors, "error", "errors"),
            count(warnings, "warning", "warnings")
        ),
    }
}

/// The badge of a module with `counts` — its errors and warnings, as
/// `ScriptProblems::of_module` gives them; `None` with neither. An error
/// outranks any number of warnings.
pub(crate) fn tile_badge(counts: (usize, usize)) -> Option<TileBadge> {
    match counts {
        (0, 0) => None,
        (0, warnings) => Some(TileBadge {
            severity: ProblemSeverity::Warning,
            count: warnings,
        }),
        (errors, _) => Some(TileBadge {
            severity: ProblemSeverity::Error,
            count: errors,
        }),
    }
}

/// Where a project keeps its scripts, under its assets root.
pub(crate) const SCRIPT_DIR: &str = "scripts";

/// The module an asset at `rel_path` (under the assets root) is known by,
/// `None` when it is not an `.erg` file.
pub(crate) fn module_of_asset(rel_path: &str) -> Option<String> {
    let path = rel_path.replace('\\', "/");
    if !path.ends_with(".erg") {
        return None;
    }
    Some(
        path.strip_prefix(&format!("{SCRIPT_DIR}/"))
            .unwrap_or(&path)
            .to_owned(),
    )
}

/// The file of `module` in the project whose assets are at `assets_root`.
pub(crate) fn module_file(assets_root: &Path, module: &str) -> PathBuf {
    module
        .split('/')
        .fold(assets_root.join(SCRIPT_DIR), |path, part| path.join(part))
}
