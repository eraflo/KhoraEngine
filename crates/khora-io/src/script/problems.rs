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

//! What compiling a module found, reported to the project's
//! [`Problems`] — located through the program's source map, each
//! problem naming the module it is in — and the `.erg` files an assets root
//! holds outside its script folder, which are never compiled.

use std::path::Path;

use khora_core::problems::{Problem, ProblemSeverity, ProblemSource, Problems};
use khora_script::diagnostics::Severity;
use khora_script::CompileOutcome;

use khora_script::modules::EXTENSION;

use crate::script::layout::{misplaced, SCRIPT_DIR};

/// Reports what compiling `module` found, replacing what its last compile
/// reported — none clears it. Each problem names the module it is located in,
/// which may be one `module` imports: compiling that module may find it too,
/// and the list shows it once; whichever compile found it is the one whose
/// clean compile takes it back.
pub fn report_compile(problems: &Problems, module: &str, outcome: &CompileOutcome) {
    let found = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let location = outcome.sources.locate(diagnostic.span);
            let (owner, line, column) = match location {
                Some(location) => (location.module, location.line, location.column),
                None => (module.to_owned(), 0, 0),
            };
            Problem {
                severity: match diagnostic.severity {
                    Severity::Error => ProblemSeverity::Error,
                    Severity::Warning => ProblemSeverity::Warning,
                },
                module: Some(owner),
                line,
                column,
                message: diagnostic.message.clone(),
                note: diagnostic.note.clone(),
                entity: None,
                behavior: None,
                component: None,
                field: None,
            }
        })
        .collect();
    problems.report(
        ProblemSource::Compile {
            module: module.to_owned(),
        },
        found,
    );
}

/// The problem of the Ergon file at `rel_path` (under its assets root,
/// forward slashes) that is no module: an error in no module, at no line, its
/// message naming the path, its note saying what to do — move it to
/// `scripts/<rel_path>`; when its first folder is `scripts` spelled in another
/// case, rename that folder `scripts`; when it is in place but spelled `.ERG`,
/// rename it `.erg`. Every path it suggests ends in `.erg`.
pub fn misplaced_problem(rel_path: &str) -> Problem {
    let first = rel_path.split('/').next().unwrap_or(rel_path);
    let extension = rel_path.rsplit('.').next().unwrap_or_default();
    let fixed = format!(
        "{}{EXTENSION}",
        &rel_path[..rel_path.len() - extension.len()]
    );
    let spelled_wrong = extension != EXTENSION;
    let (message, note) = if rel_path.starts_with(&format!("{SCRIPT_DIR}/")) {
        (
            format!("`{rel_path}` is not compiled: a script's extension is `.{EXTENSION}`, in lower case"),
            format!("Rename it `{fixed}` — the one spelling an import accepts."),
        )
    } else if rel_path.contains('/') && first.eq_ignore_ascii_case(SCRIPT_DIR) {
        (
            format!("`{rel_path}` is not compiled: scripts live in `{SCRIPT_DIR}/`"),
            format!(
                "Rename the folder `{first}` to `{SCRIPT_DIR}`, in lower case{}: the script \
                 folder is matched exactly, so the project runs the same on every system.",
                if spelled_wrong {
                    format!(", and the file's extension to `.{EXTENSION}`")
                } else {
                    String::new()
                }
            ),
        )
    } else {
        (
            format!("`{rel_path}` is not compiled: scripts live in `{SCRIPT_DIR}/`"),
            format!(
                "Move it to `{SCRIPT_DIR}/{fixed}` — the one folder scripts are compiled from, \
                 and the folder its imports are written from."
            ),
        )
    };
    Problem {
        severity: ProblemSeverity::Error,
        module: None,
        line: 0,
        column: 0,
        message,
        note: Some(note),
        entity: None,
        behavior: None,
        component: None,
        field: None,
    }
}

/// Reports every `.erg` file under `assets_root` outside its script folder,
/// under [`ProblemSource::Placement`] for that root — replacing what the root
/// reported before; none clears it.
pub fn report_misplaced(problems: &Problems, assets_root: &Path) {
    problems.report(
        placement_of(assets_root),
        misplaced(assets_root)
            .iter()
            .map(|rel_path| misplaced_problem(rel_path))
            .collect(),
    );
}

/// Takes back what [`report_misplaced`] reported for `assets_root`: the root
/// is no longer the project's to show.
pub fn clear_misplaced(problems: &Problems, assets_root: &Path) {
    problems.report(placement_of(assets_root), Vec::new());
}

/// The source `assets_root`'s misplaced files are reported under — keyed by
/// the root made absolute, as the watch reports it, so the mount that names
/// the root and the pump that hears of its changes replace one list, not two.
fn placement_of(assets_root: &Path) -> ProblemSource {
    ProblemSource::Placement {
        root: crate::asset::root_path::absolute(assets_root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes an empty script at `rel_path` under `root`, making its folders.
    fn touch(root: &Path, rel_path: &str) {
        let path = root.join(rel_path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("makes the folders");
        std::fs::write(&path, "fn void Main() { }").expect("writes");
    }

    /// The problems held in no module that name `rel_path`.
    fn naming(problems: &Problems, rel_path: &str) -> Vec<Problem> {
        problems
            .all()
            .into_iter()
            .filter(|problem| problem.module.is_none() && problem.message.contains(rel_path))
            .collect()
    }

    // ─── misplaced_problem ──────────────────────────────────────────────────

    /// **A misplaced file is an error** — it does not run — located in no
    /// module and at no line, its message naming the file.
    #[test]
    fn a_misplaced_file_is_an_error_in_no_module_naming_its_path() {
        let problem = misplaced_problem("props/lantern.erg");

        assert_eq!(problem.severity, ProblemSeverity::Error);
        assert_eq!(problem.module, None);
        assert_eq!((problem.line, problem.column), (0, 0));
        assert!(
            problem.message.contains("props/lantern.erg"),
            "{}",
            problem.message
        );
        assert_eq!(problem.entity, None);
        assert_eq!(problem.behavior, None);
    }

    /// **The note says where it belongs**: the same path under `scripts/`,
    /// which keeps the imports it is written with meaning what they meant.
    #[test]
    fn a_misplaced_file_is_told_to_move_under_the_script_folder() {
        let note = misplaced_problem("props/lantern.erg")
            .note
            .expect("a note on what to do");

        assert!(note.contains("scripts/props/lantern.erg"), "{note}");
    }

    #[test]
    fn a_file_at_the_top_of_the_assets_is_told_to_move_under_the_script_folder() {
        let note = misplaced_problem("guard.erg")
            .note
            .expect("a note on what to do");

        assert!(note.contains("scripts/guard.erg"), "{note}");
    }

    /// **A script folder spelled in another case is renamed, not nested**:
    /// moving `Scripts/ai/guard.erg` to `scripts/Scripts/ai/guard.erg` is not
    /// what its author meant.
    #[test]
    fn a_script_folder_in_another_case_is_told_to_be_renamed() {
        for rel_path in ["Scripts/ai/guard.erg", "SCRIPTS/guard.erg"] {
            let folder = rel_path.split('/').next().expect("a first folder");
            let note = misplaced_problem(rel_path)
                .note
                .expect("a note on what to do");

            assert!(note.to_lowercase().contains("rename"), "{note}");
            assert!(note.contains(folder), "names the folder: {note}");
            assert!(
                !note.contains(&format!("scripts/{rel_path}")),
                "not moved under itself: {note}"
            );
        }
    }

    /// A folder whose name merely starts with `scripts` is another folder: its
    /// file moves.
    #[test]
    fn a_folder_only_starting_with_scripts_is_told_to_move() {
        let note = misplaced_problem("scriptsextra/guard.erg")
            .note
            .expect("a note on what to do");

        assert!(note.contains("scripts/scriptsextra/guard.erg"), "{note}");
    }

    // ─── report_misplaced ───────────────────────────────────────────────────

    /// **One error per misplaced file**, and none for the scripts in place.
    #[test]
    fn report_misplaced_reports_one_error_per_misplaced_file() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "scripts/main.erg");
        touch(root.path(), "props/lantern.erg");
        touch(root.path(), "guard.erg");
        let problems = Problems::default();

        report_misplaced(&problems, root.path());

        let all = problems.all();
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(naming(&problems, "props/lantern.erg").len(), 1, "{all:?}");
        assert!(
            all.iter()
                .any(|problem| problem.message.contains("guard.erg")
                    && !problem.message.contains("lantern")),
            "{all:?}"
        );
        assert!(
            all.iter()
                .all(|problem| problem.severity == ProblemSeverity::Error),
            "{all:?}"
        );
    }

    /// **A report replaces the root's last one**: a file moved into place
    /// leaves, a file newly misplaced comes.
    #[test]
    fn report_misplaced_replaces_what_the_root_reported_before() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "props/lantern.erg");
        let problems = Problems::default();
        report_misplaced(&problems, root.path());
        assert_eq!(
            naming(&problems, "props/lantern.erg").len(),
            1,
            "the premise"
        );

        std::fs::remove_file(root.path().join("props/lantern.erg")).expect("removes");
        touch(root.path(), "props/torch.erg");
        report_misplaced(&problems, root.path());

        assert!(naming(&problems, "props/lantern.erg").is_empty());
        assert_eq!(naming(&problems, "props/torch.erg").len(), 1);
        assert_eq!(problems.all().len(), 1, "{:?}", problems.all());
    }

    /// **Nothing misplaced clears the root**, under the source that names it.
    #[test]
    fn report_misplaced_with_nothing_misplaced_clears_the_root() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "scripts/main.erg");
        let problems = Problems::default();
        problems.report(
            ProblemSource::Placement {
                root: root.path().to_path_buf(),
            },
            vec![Problem {
                severity: ProblemSeverity::Error,
                module: None,
                line: 0,
                column: 0,
                message: "`old.erg` was misplaced".to_owned(),
                note: None,
                entity: None,
                behavior: None,
                component: None,
                field: None,
            }],
        );

        report_misplaced(&problems, root.path());

        assert!(problems.all().is_empty(), "{:?}", problems.all());
    }

    /// **Each root has its own list**: clearing one leaves another's alone,
    /// and the other sources too.
    #[test]
    fn report_misplaced_leaves_other_roots_and_sources_alone() {
        let first = tempfile::TempDir::new().expect("a temp dir");
        let second = tempfile::TempDir::new().expect("a temp dir");
        touch(first.path(), "props/lantern.erg");
        touch(second.path(), "props/torch.erg");
        let problems = Problems::default();
        problems.report(
            ProblemSource::Compile {
                module: "main.erg".to_owned(),
            },
            vec![Problem {
                severity: ProblemSeverity::Warning,
                module: Some("main.erg".to_owned()),
                line: 1,
                column: 1,
                message: "a warning".to_owned(),
                note: None,
                entity: None,
                behavior: None,
                component: None,
                field: None,
            }],
        );
        report_misplaced(&problems, first.path());
        report_misplaced(&problems, second.path());

        std::fs::remove_file(first.path().join("props/lantern.erg")).expect("removes");
        report_misplaced(&problems, first.path());

        assert!(naming(&problems, "props/lantern.erg").is_empty());
        assert_eq!(naming(&problems, "props/torch.erg").len(), 1);
        assert_eq!(problems.of_module("main.erg"), (0, 1));
    }

    /// A root that is gone has nothing misplaced.
    #[test]
    fn report_misplaced_over_a_missing_root_reports_nothing() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        let problems = Problems::default();

        report_misplaced(&problems, &root.path().join("gone"));

        assert!(problems.all().is_empty());
    }

    /// **Clearing takes back what a report put there**, and only that root's.
    #[test]
    fn clear_misplaced_takes_back_the_roots_list() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        let other = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "props/lantern.erg");
        touch(other.path(), "props/torch.erg");
        let problems = Problems::default();
        report_misplaced(&problems, root.path());
        report_misplaced(&problems, other.path());

        clear_misplaced(&problems, root.path());

        assert!(naming(&problems, "props/lantern.erg").is_empty());
        assert_eq!(naming(&problems, "props/torch.erg").len(), 1);
    }

    /// **A file in place spelled `.ERG` is told to rename its extension**, and
    /// every path a note suggests is one a module can have.
    #[test]
    fn a_capital_extension_is_told_the_spelling_it_needs() {
        let in_place = misplaced_problem("scripts/ai/Guard.ERG");
        let note = in_place.note.expect("a note");
        assert!(note.contains("scripts/ai/Guard.erg"), "{note}");

        let elsewhere = misplaced_problem("props/Lantern.Erg");
        let note = elsewhere.note.expect("a note");
        assert!(note.contains("scripts/props/Lantern.erg"), "{note}");
        assert!(!note.contains(".Erg`"), "{note}");
    }
}
