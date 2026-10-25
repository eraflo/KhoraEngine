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
//! engine's `Problems` without drawing anything.

use std::path::{Path, PathBuf};

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity};

/// One module's problems — or, for the scene's data, one component's — as
/// the tab lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProblemGroup {
    /// The module, `None` for the problems in no module.
    pub module: Option<String>,
    /// For problems in no module, the component they are about ("Camera"),
    /// `None` for those about none.
    pub component: Option<String>,
    /// Its problems, in the order given.
    pub rows: Vec<ProblemRow>,
}

impl ProblemGroup {
    /// What the group's header reads: its module, or the component its
    /// problems are in ("Camera") for the scene's data problems.
    pub(crate) fn title(&self) -> String {
        self.module
            .clone()
            .or_else(|| self.component.clone())
            .unwrap_or_else(|| "Not in a script".to_owned())
    }
}

/// One problem, as the tab lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProblemRow {
    /// Error or warning.
    pub severity: ProblemSeverity,
    /// `line:col  message`, or the message alone when it has no position;
    /// for a problem in a component's field, `Entity · field — message`.
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
/// in name order, then the scene's data problems by component, the problems
/// in neither last, each group's rows in the order given.
pub(crate) fn grouped(
    problems: &[Problem],
    name_of: &dyn Fn(EntityId) -> Option<String>,
) -> Vec<ProblemGroup> {
    let mut groups: Vec<ProblemGroup> = Vec::new();
    let name =
        |entity: EntityId| name_of(entity).unwrap_or_else(|| format!("Entity {}", entity.index));
    for problem in problems {
        // A component's problem names the entity in its text: the row is
        // about that entity's value, not something that happened on it.
        let in_field = problem.module.is_none() && problem.component.is_some();
        let text = match (&problem.field, problem.entity) {
            (Some(field), Some(entity)) if in_field => {
                format!("{} · {field} — {}", name(entity), problem.message)
            }
            (Some(field), None) if in_field => format!("{field} — {}", problem.message),
            (None, Some(entity)) if in_field => {
                format!("{} — {}", name(entity), problem.message)
            }
            _ if problem.line == 0 => problem.message.clone(),
            _ => format!("{}:{}  {}", problem.line, problem.column, problem.message),
        };
        let row = ProblemRow {
            severity: problem.severity,
            text,
            note: problem.note.clone(),
            entity: problem.entity.filter(|_| !in_field).map(name),
            action: row_action(problem),
        };
        let component = if problem.module.is_none() {
            problem.component.clone()
        } else {
            None
        };
        match groups
            .iter_mut()
            .find(|group| group.module == problem.module && group.component == component)
        {
            Some(group) => group.rows.push(row),
            None => groups.push(ProblemGroup {
                module: problem.module.clone(),
                component,
                rows: vec![row],
            }),
        }
    }
    // Modules first, by name; then components, by name; then the rest. (A
    // bare `None` would sort first as an `Option`.)
    groups.sort_by(|a, b| {
        let key = |g: &ProblemGroup| {
            (
                g.module.is_none(),
                g.module.clone(),
                g.component.is_none(),
                g.component.clone(),
            )
        };
        key(a).cmp(&key(b))
    });
    groups
}

/// What clicking `problem`'s row does: a running problem selects its entity,
/// any other in a module opens that module.
pub(crate) fn row_action(problem: &Problem) -> ProblemAction {
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
pub(crate) fn tab_title(problems: &[Problem]) -> String {
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
/// `Problems::of_module` gives them; `None` with neither. An error
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

/// Where a project keeps its scripts, under its assets root — the engine's
/// one rule, not a copy of it.
pub(crate) const SCRIPT_DIR: &str = khora_sdk::scripts::SCRIPT_DIR;

/// The module an asset at `rel_path` (under the assets root) is known by,
/// `None` when it is no script: not an `.erg` file, or one outside the script
/// folder.
pub(crate) fn module_of_asset(rel_path: &str) -> Option<String> {
    let path = rel_path.replace('\\', "/");
    khora_sdk::scripts::module_of(&path).map(str::to_owned)
}

/// What the tile of the asset at `rel_path` (under the assets root) counts,
/// as (errors, warnings): a script's are its module's in `problems`; a
/// misplaced `.erg` — one outside the script folder, never compiled — has its
/// one error; any other file has no count.
pub(crate) fn script_tile_counts(rel_path: &str, problems: &[Problem]) -> Option<(usize, usize)> {
    let Some(module) = module_of_asset(rel_path) else {
        return khora_sdk::scripts::is_script_file(rel_path).then_some((1, 0));
    };
    Some(
        problems
            .iter()
            .filter(|problem| problem.module.as_deref() == Some(module.as_str()))
            .fold((0, 0), |(errors, warnings), problem| {
                match problem.severity {
                    ProblemSeverity::Error => (errors + 1, warnings),
                    ProblemSeverity::Warning => (errors, warnings + 1),
                }
            }),
    )
}

/// The file of `module` in the project whose assets are at `assets_root`.
pub(crate) fn module_file(assets_root: &Path, module: &str) -> PathBuf {
    module
        .split('/')
        .fold(assets_root.join(SCRIPT_DIR), |path, part| path.join(part))
}
