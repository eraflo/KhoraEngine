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

//! **An `.erg` tile follows the engine's one rule**: a file under the exact
//! `scripts/` folder is the module named by its path after it, and wears that
//! module's counts; an `.erg` anywhere else is no module — it is never
//! compiled — and wears the one error the Problems tab lists for it.

use khora_sdk::khora_core::script::{ProblemSeverity, ScriptProblem};

use crate::panels::problems::view::{module_of_asset, script_tile_counts};

fn problem(severity: ProblemSeverity, module: &str, message: &str) -> ScriptProblem {
    ScriptProblem {
        severity,
        module: Some(module.to_owned()),
        line: 1,
        column: 1,
        message: message.to_owned(),
        note: None,
        entity: None,
        behavior: None,
    }
}

/// `ai/guard.erg` with an error and two warnings, `lantern.erg` — the name
/// a misplaced `props/lantern.erg` would take under another rule — with an
/// error, and another module with one.
fn reported() -> Vec<ScriptProblem> {
    vec![
        problem(ProblemSeverity::Error, "ai/guard.erg", "one"),
        problem(ProblemSeverity::Warning, "ai/guard.erg", "two"),
        problem(ProblemSeverity::Warning, "ai/guard.erg", "three"),
        problem(ProblemSeverity::Error, "lantern.erg", "elsewhere"),
        problem(ProblemSeverity::Error, "other.erg", "unrelated"),
    ]
}

// ─── module_of_asset ────────────────────────────────────────────────────────

/// **An `.erg` outside `scripts/` is no module**: the tile must not look up
/// a module the engine never compiled.
#[test]
fn an_erg_outside_the_script_folder_is_no_module() {
    assert_eq!(module_of_asset("props/lantern.erg"), None);
    assert_eq!(module_of_asset("guard.erg"), None);
    assert_eq!(module_of_asset("Scripts/guard.erg"), None);
    assert_eq!(module_of_asset("props/scripts/x.erg"), None);
}

/// A path written with backslashes names the same module.
#[test]
fn a_tile_path_with_backslashes_names_the_same_module() {
    assert_eq!(
        module_of_asset("scripts\\ai\\guard.erg"),
        Some("ai/guard.erg".to_owned())
    );
    assert_eq!(module_of_asset("props\\lantern.erg"), None);
}

// ─── script_tile_counts ─────────────────────────────────────────────────────

/// **A script's tile counts its module's errors and warnings.**
#[test]
fn a_script_tile_counts_its_module_problems() {
    assert_eq!(
        script_tile_counts("scripts/ai/guard.erg", &reported()),
        Some((1, 2))
    );
    assert_eq!(
        script_tile_counts("scripts\\ai\\guard.erg", &reported()),
        Some((1, 2)),
        "either separator"
    );
}

/// A script with nothing wrong counts nothing — still a script's count.
#[test]
fn a_clean_script_tile_counts_nothing() {
    assert_eq!(
        script_tile_counts("scripts/rules.erg", &reported()),
        Some((0, 0))
    );
}

/// **A misplaced `.erg` wears its one error**, whatever is reported under a
/// module of its bare file name.
#[test]
fn a_misplaced_erg_tile_counts_one_error() {
    assert_eq!(
        script_tile_counts("props/lantern.erg", &reported()),
        Some((1, 0))
    );
    assert_eq!(script_tile_counts("guard.erg", &[]), Some((1, 0)));
    assert_eq!(script_tile_counts("Scripts/x.erg", &[]), Some((1, 0)));
}

/// An Ergon file spelled `.ERG` is no module — in place or not — and wears
/// the one error the engine reports for it, as a misplaced file does.
#[test]
fn an_erg_tile_with_a_capital_extension_counts_one_error() {
    assert_eq!(
        script_tile_counts(
            "scripts/Guard.ERG",
            &[problem(ProblemSeverity::Warning, "Guard.ERG", "w")]
        ),
        Some((1, 0))
    );
    assert_eq!(script_tile_counts("props/Lantern.ERG", &[]), Some((1, 0)));
}

/// **Any other file has no count**, under the script folder or not.
#[test]
fn a_tile_that_is_not_ergon_has_no_count() {
    assert_eq!(script_tile_counts("textures/wood.png", &reported()), None);
    assert_eq!(script_tile_counts("scripts/notes.txt", &reported()), None);
}
