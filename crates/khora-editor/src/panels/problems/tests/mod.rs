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

//! What the Problems tab and the `.erg` tiles show: rows grouped by module,
//! `line:col  message`, the entity named for a running problem, counts in the
//! tab's title, a badge where the worst problem wins, and where a click goes.

mod component_rows;
mod engine_mirror_rows;
mod load_report;
mod misplaced_script_tiles;

use std::path::Path;

use khora_sdk::khora_core::ecs::entity::EntityId;
use khora_sdk::khora_core::problems::{Problem, ProblemSeverity};

use super::view::{
    grouped, module_file, module_of_asset, row_action, tab_title, tile_badge, ProblemAction,
    TileBadge,
};

fn problem(
    severity: ProblemSeverity,
    module: Option<&str>,
    line: u32,
    column: u32,
    message: &str,
) -> Problem {
    Problem {
        severity,
        module: module.map(str::to_owned),
        line,
        column,
        message: message.to_owned(),
        note: None,
        entity: None,
        behavior: None,
        component: None,
        field: None,
    }
}

fn error(module: &str, line: u32, column: u32, message: &str) -> Problem {
    problem(ProblemSeverity::Error, Some(module), line, column, message)
}

fn warning(module: &str, line: u32, column: u32, message: &str) -> Problem {
    problem(
        ProblemSeverity::Warning,
        Some(module),
        line,
        column,
        message,
    )
}

/// A running problem: `behavior` on `entity` faulted, in `module`.
fn faulted(module: &str, entity: EntityId, behavior: &str, reason: &str) -> Problem {
    Problem {
        entity: Some(entity),
        behavior: Some(behavior.to_owned()),
        ..problem(ProblemSeverity::Error, Some(module), 0, 0, reason)
    }
}

fn guard_entity() -> EntityId {
    EntityId {
        index: 7,
        generation: 2,
    }
}

/// Names the guard entity "Guard 1", and no other.
fn names(entity: EntityId) -> Option<String> {
    (entity == guard_entity()).then(|| "Guard 1".to_owned())
}

// ─── Rows ───────────────────────────────────────────────────────────────────

/// **Rows are grouped by module**, each module once, in module order, with
/// the problems in no module last.
#[test]
fn rows_are_grouped_by_module_in_module_order() {
    let orphan = problem(
        ProblemSeverity::Warning,
        None,
        0,
        0,
        "`Health` is declared by no module",
    );
    let problems = vec![
        error("b.erg", 1, 1, "b one"),
        error("a.erg", 2, 1, "a one"),
        warning("b.erg", 3, 1, "b two"),
        orphan,
    ];

    let groups = grouped(&problems, &names);

    let modules: Vec<Option<&str>> = groups.iter().map(|g| g.module.as_deref()).collect();
    assert_eq!(modules, vec![Some("a.erg"), Some("b.erg"), None]);
    assert_eq!(groups[1].rows.len(), 2, "both of b.erg's, in one group");
}

/// **A row reads `line:col  message`**, in the order the problems came, its
/// note beside it.
#[test]
fn a_row_reads_line_colon_column_then_the_message() {
    let mut first = error("guard.erg", 2, 16, "expected `float`, found `bool`");
    first.note = Some("a bool is not a float".to_owned());
    let problems = vec![first, warning("guard.erg", 3, 10, "reserved")];

    let groups = grouped(&problems, &names);

    assert_eq!(groups.len(), 1);
    let rows = &groups[0].rows;
    assert_eq!(rows[0].text, "2:16  expected `float`, found `bool`");
    assert_eq!(rows[0].note.as_deref(), Some("a bool is not a float"));
    assert_eq!(rows[0].severity, ProblemSeverity::Error);
    assert_eq!(rows[1].text, "3:10  reserved");
    assert_eq!(rows[1].severity, ProblemSeverity::Warning);
}

/// **A problem with no position reads as its message alone** — no `0:0`.
#[test]
fn a_row_without_a_position_is_its_message_alone() {
    let problems = vec![error("guard.erg", 0, 0, "`Health` is declared by both")];

    let groups = grouped(&problems, &names);

    assert_eq!(groups[0].rows[0].text, "`Health` is declared by both");
}

/// **A running problem names its entity.**
#[test]
fn a_running_problem_names_its_entity() {
    let problems = vec![faulted(
        "guard.erg",
        guard_entity(),
        "Guard",
        "integer division by zero",
    )];

    let groups = grouped(&problems, &names);

    assert_eq!(groups[0].module.as_deref(), Some("guard.erg"));
    assert_eq!(groups[0].rows[0].entity.as_deref(), Some("Guard 1"));
}

/// **An entity with no name is still named** — by something carrying its
/// index — rather than left blank.
#[test]
fn an_unnamed_entity_is_named_by_its_index() {
    let unnamed = EntityId {
        index: 41,
        generation: 1,
    };
    let problems = vec![faulted("guard.erg", unnamed, "Guard", "faulted")];

    let groups = grouped(&problems, &names);

    let label = groups[0].rows[0]
        .entity
        .clone()
        .expect("a running problem names its entity");
    assert!(label.contains("41"), "{label}");
}

/// A compile problem names no entity.
#[test]
fn a_compile_problem_names_no_entity() {
    let groups = grouped(&[error("guard.erg", 1, 1, "broken")], &names);

    assert_eq!(groups[0].rows[0].entity, None);
}

// ─── Where a click goes ─────────────────────────────────────────────────────

/// **A problem in a module opens that module's file.**
#[test]
fn a_problem_in_a_module_opens_the_module() {
    assert_eq!(
        row_action(&error("ai/guard.erg", 2, 16, "broken")),
        ProblemAction::OpenModule("ai/guard.erg".to_owned())
    );
}

/// **A running problem selects its entity**, though it sits in a module too.
#[test]
fn a_running_problem_selects_its_entity() {
    assert_eq!(
        row_action(&faulted("guard.erg", guard_entity(), "Guard", "faulted")),
        ProblemAction::SelectEntity(guard_entity())
    );
}

/// A problem in no module and on no entity goes nowhere.
#[test]
fn a_problem_in_no_module_on_no_entity_goes_nowhere() {
    let orphan = problem(ProblemSeverity::Warning, None, 0, 0, "orphaned");

    assert_eq!(row_action(&orphan), ProblemAction::Nothing);
}

/// **The grouped rows carry where they go**, as `row_action` says.
#[test]
fn a_grouped_row_carries_its_action() {
    let problems = vec![
        error("guard.erg", 1, 1, "broken"),
        faulted("guard.erg", guard_entity(), "Guard", "faulted"),
    ];

    let groups = grouped(&problems, &names);

    let actions: Vec<&ProblemAction> = groups[0].rows.iter().map(|row| &row.action).collect();
    assert_eq!(
        actions,
        vec![
            &ProblemAction::OpenModule("guard.erg".to_owned()),
            &ProblemAction::SelectEntity(guard_entity()),
        ]
    );
}

/// **A module's file is under the project's script folder**.
#[test]
fn a_modules_file_is_under_the_script_folder() {
    let assets = Path::new("project").join("assets");

    assert_eq!(
        module_file(&assets, "ai/guard.erg"),
        assets.join("scripts").join("ai").join("guard.erg")
    );
}

// ─── The tab ────────────────────────────────────────────────────────────────

/// **With nothing wrong the tab is just "Problems".**
#[test]
fn the_tab_with_nothing_wrong_is_titled_problems() {
    assert_eq!(tab_title(&[]), "Problems");
}

/// **The tab's title counts the errors and the warnings**, and tells two
/// errors and a warning from an error and two warnings.
#[test]
fn the_tab_title_counts_errors_and_warnings() {
    let two_errors = vec![
        error("a.erg", 1, 1, "one"),
        error("b.erg", 1, 1, "two"),
        warning("a.erg", 2, 1, "three"),
    ];
    let two_warnings = vec![
        error("a.erg", 1, 1, "one"),
        warning("b.erg", 1, 1, "two"),
        warning("a.erg", 2, 1, "three"),
    ];

    let title = tab_title(&two_errors);
    assert!(title.starts_with("Problems"), "{title}");
    assert!(title.contains('2') && title.contains('1'), "{title}");
    assert_ne!(title, tab_title(&two_warnings));
}

// ─── The tile badge ─────────────────────────────────────────────────────────

/// **A module with nothing wrong wears no badge.**
#[test]
fn a_clean_module_wears_no_badge() {
    assert_eq!(tile_badge((0, 0)), None);
}

/// **An error beats a warning**: a module with both wears its errors.
#[test]
fn an_error_badge_beats_warnings() {
    assert_eq!(
        tile_badge((1, 3)),
        Some(TileBadge {
            severity: ProblemSeverity::Error,
            count: 1,
        })
    );
}

/// A module with only warnings wears them.
#[test]
fn a_module_with_only_warnings_wears_a_warning_badge() {
    assert_eq!(
        tile_badge((0, 2)),
        Some(TileBadge {
            severity: ProblemSeverity::Warning,
            count: 2,
        })
    );
}

/// **A tile finds its module from its asset path**: the path under the
/// script folder, as `of_module` names it.
#[test]
fn an_erg_tile_is_known_by_its_path_under_the_script_folder() {
    assert_eq!(
        module_of_asset("scripts/ai/guard.erg"),
        Some("ai/guard.erg".to_owned())
    );
    assert_eq!(module_of_asset("textures/wood.png"), None);
}
