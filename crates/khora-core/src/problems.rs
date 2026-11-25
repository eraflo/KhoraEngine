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

//! What is wrong with the project, for a tool to show.
//!
//! Several places find a problem and none of them can see the others. For
//! scripts: the compile path (a module that does not compile, or compiles
//! with warnings), the registration of what modules declare (a name two
//! modules declare, a declaration deleted), the relayout of a module's
//! components (a stored value their new fields cannot keep), the script lane (an instance that
//! faulted, a body that runs too long), and the scan of an assets root for
//! `.erg` files that sit outside its `scripts/` folder and so are never
//! compiled. For the scene's data: the components' own checks (a camera whose
//! near plane sits at its eye), and what the last scene load had to adapt.
//! Each reports its own list here, already located, and replaces it each time
//! it reports.

use crate::ecs::entity::EntityId;
use crate::sync::RwLockExt;

/// How much a problem matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProblemSeverity {
    /// Something does not compile, register or run.
    Error,
    /// It runs, but something looks wrong.
    Warning,
}

/// Who found a problem: each source replaces its own list when it reports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProblemSource {
    /// The compile of one module.
    Compile {
        /// The module compiled, by its path under the script root.
        module: String,
    },
    /// The registration of what the modules declare.
    Registration,
    /// The last relayout of the components one module declares: each value a
    /// stored row could not keep when their fields changed.
    Relayout {
        /// The module declaring them, by its path under the script root.
        module: String,
    },
    /// The instances running.
    Running,
    /// The scan of an assets root for `.erg` files outside its `scripts/`
    /// folder — files that are never compiled.
    Placement {
        /// The assets root scanned, as it was mounted.
        root: std::path::PathBuf,
    },
    /// The components' own checks, run on the world as it is now.
    Data,
    /// What the last scene load reported.
    Load,
    /// What the last save of a scene or a prefab left out of its file: each
    /// value only the session holds, such as a mesh made in code.
    Save,
    /// The engine's startup: the work it waited on that had not landed when
    /// its wait timed out.
    Startup,
    /// The files the engine refused to read, or quarantined.
    ///
    /// A path leaving its root, a file over its size cap, bytes that are not
    /// what their extension says.
    Security,
    /// What a restricted project does not run: a shader overlay ignored, as
    /// warnings; cleared once the project is trusted.
    Restricted,
}

/// One problem, located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Error or warning.
    pub severity: ProblemSeverity,
    /// The module it is in, when it is in one.
    pub module: Option<String>,
    /// 1-based line, 0 when it has none.
    pub line: u32,
    /// 1-based column in characters, 0 when it has none.
    pub column: u32,
    /// What is wrong.
    pub message: String,
    /// Why, or what to do about it.
    pub note: Option<String>,
    /// The entity it happened on, for a running problem.
    pub entity: Option<EntityId>,
    /// The behavior it happened in, for a running problem.
    pub behavior: Option<String>,
    /// The component it is about, for a problem in the scene's data.
    pub component: Option<String>,
    /// The field of that component, as the dotted path of its serialized
    /// form (`z_near`, `projection.Perspective.fov_y_radians`).
    pub field: Option<String>,
}

impl Problem {
    /// Returns a `severity` problem saying `message`, located nowhere, every other field empty.
    pub fn new(severity: ProblemSeverity, message: impl Into<String>) -> Self {
        Self {
            severity,
            module: None,
            line: 0,
            column: 0,
            message: message.into(),
            note: None,
            entity: None,
            behavior: None,
            component: None,
            field: None,
        }
    }

    /// Returns a `severity` problem in the file `module` at `line`:`column` (0 for none).
    pub fn of_file(
        severity: ProblemSeverity,
        module: impl Into<String>,
        line: u32,
        column: u32,
        message: impl Into<String>,
    ) -> Self {
        Self {
            module: Some(module.into()),
            line,
            column,
            ..Self::new(severity, message)
        }
    }
}

/// Every source's problems. Shared as an `Arc` in the runtime's resources.
#[derive(Debug, Default)]
pub struct Problems(std::sync::RwLock<Held>);

impl Problems {
    /// Replaces what `source` reported last with `problems`; an empty list
    /// clears it.
    pub fn report(&self, source: ProblemSource, problems: Vec<Problem>) {
        let mut held = self.0.write_recovered();
        let unchanged = match held.by_source.get(&source) {
            Some(before) => *before == problems,
            None => problems.is_empty(),
        };
        if unchanged {
            return;
        }
        if problems.is_empty() {
            held.by_source.remove(&source);
        } else {
            held.by_source.insert(source, problems);
        }
        held.generation += 1;
    }

    /// Returns every problem of every source, each once, errors first.
    ///
    /// Then sorted by module, line, column, component and field; two compiles
    /// finding one problem in a shared module list it once.
    pub fn all(&self) -> Vec<Problem> {
        let held = self.0.read_recovered();
        let mut all: Vec<Problem> = held.by_source.values().flatten().cloned().collect();
        all.sort_by(|a, b| {
            (
                rank(a.severity),
                &a.module,
                a.line,
                a.column,
                &a.component,
                &a.field,
                &a.message,
            )
                .cmp(&(
                    rank(b.severity),
                    &b.module,
                    b.line,
                    b.column,
                    &b.component,
                    &b.field,
                    &b.message,
                ))
        });
        all.dedup();
        all
    }

    /// Returns what `source` reported last, as it reported it; empty when it holds
    /// nothing.
    pub fn of_source(&self, source: &ProblemSource) -> Vec<Problem> {
        let held = self.0.read_recovered();
        held.by_source.get(source).cloned().unwrap_or_default()
    }

    /// Returns how many errors and warnings are located in `module`.
    pub fn of_module(&self, module: &str) -> (usize, usize) {
        self.all()
            .iter()
            .filter(|problem| problem.module.as_deref() == Some(module))
            .fold((0, 0), |(errors, warnings), problem| {
                match problem.severity {
                    ProblemSeverity::Error => (errors + 1, warnings),
                    ProblemSeverity::Warning => (errors, warnings + 1),
                }
            })
    }

    /// Moves each time a report changes what is held, so a reader can skip a
    /// frame where nothing did.
    pub fn generation(&self) -> u64 {
        self.0.read_recovered().generation
    }
}

/// What is held: each source's list, and how many changes there were.
#[derive(Debug, Default)]
struct Held {
    by_source: std::collections::HashMap<ProblemSource, Vec<Problem>>,
    generation: u64,
}

/// Errors before warnings.
fn rank(severity: ProblemSeverity) -> u8 {
    match severity {
        ProblemSeverity::Error => 0,
        ProblemSeverity::Warning => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(module: &str) -> ProblemSource {
        ProblemSource::Compile {
            module: module.to_owned(),
        }
    }

    /// A problem of `severity` at `module:line:column`.
    fn problem(
        severity: ProblemSeverity,
        module: &str,
        line: u32,
        column: u32,
        message: &str,
    ) -> Problem {
        Problem::of_file(severity, module, line, column, message)
    }

    fn error(module: &str, line: u32, column: u32, message: &str) -> Problem {
        problem(ProblemSeverity::Error, module, line, column, message)
    }

    fn warning(module: &str, line: u32, column: u32, message: &str) -> Problem {
        problem(ProblemSeverity::Warning, module, line, column, message)
    }

    fn messages(problems: &Problems) -> Vec<String> {
        problems.all().into_iter().map(|p| p.message).collect()
    }

    /// Nothing reported, nothing held.
    #[test]
    fn nothing_reported_holds_no_problem() {
        let problems = Problems::default();

        assert!(problems.all().is_empty());
        assert_eq!(problems.of_module("guard.erg"), (0, 0));
    }

    /// **What a source reports is what is held**, every field as given.
    #[test]
    fn a_reported_problem_is_held_as_given() {
        let problems = Problems::default();
        let mut reported = error("guard.erg", 3, 7, "expected `float`, found `bool`");
        reported.note = Some("a bool is not a float".to_owned());

        problems.report(compile("guard.erg"), vec![reported.clone()]);

        assert_eq!(problems.all(), vec![reported]);
    }

    /// **A source's report replaces its last one**: the module saved again
    /// with another error has that error, not both.
    #[test]
    fn a_report_replaces_what_the_same_source_reported_before() {
        let problems = Problems::default();
        problems.report(compile("guard.erg"), vec![error("guard.erg", 3, 7, "old")]);

        problems.report(compile("guard.erg"), vec![error("guard.erg", 9, 1, "new")]);

        assert_eq!(messages(&problems), vec!["new".to_owned()]);
    }

    /// **An empty report clears the source**: a clean compile leaves its
    /// module with no problem.
    #[test]
    fn an_empty_report_clears_the_source() {
        let problems = Problems::default();
        problems.report(
            compile("guard.erg"),
            vec![error("guard.erg", 3, 7, "broken")],
        );

        problems.report(compile("guard.erg"), Vec::new());

        assert!(problems.all().is_empty());
        assert_eq!(problems.of_module("guard.erg"), (0, 0));
    }

    /// **Sources are independent**: a module compiling clean leaves another
    /// module's errors, the registration's and the running ones alone.
    #[test]
    fn a_report_leaves_every_other_source_alone() {
        let problems = Problems::default();
        problems.report(
            compile("guard.erg"),
            vec![error("guard.erg", 1, 1, "guard")],
        );
        problems.report(compile("lamp.erg"), vec![error("lamp.erg", 1, 1, "lamp")]);
        problems.report(
            ProblemSource::Registration,
            vec![error("guard.erg", 0, 0, "registration")],
        );
        problems.report(
            ProblemSource::Running,
            vec![warning("guard.erg", 0, 0, "running")],
        );

        problems.report(compile("guard.erg"), Vec::new());

        let mut held = messages(&problems);
        held.sort();
        assert_eq!(held, vec!["lamp", "registration", "running"]);
    }

    /// **A source's problems read back as it reported them**, in its order; a
    /// source that reported nothing holds nothing.
    #[test]
    fn of_source_reads_back_one_sources_report_as_given() {
        let problems = Problems::default();
        let reported = vec![
            warning("guard.erg", 9, 1, "second"),
            error("guard.erg", 1, 1, "first"),
        ];
        problems.report(compile("guard.erg"), reported.clone());
        problems.report(compile("lamp.erg"), vec![error("lamp.erg", 1, 1, "lamp")]);

        assert_eq!(problems.of_source(&compile("guard.erg")), reported);
        assert!(problems.of_source(&ProblemSource::Running).is_empty());
    }

    /// **Errors come first, then by module, line and column** — whatever
    /// order the sources reported in.
    #[test]
    fn all_lists_errors_first_then_by_module_line_and_column() {
        let problems = Problems::default();
        problems.report(
            compile("b.erg"),
            vec![
                warning("b.erg", 1, 1, "b warning"),
                error("b.erg", 4, 2, "b 4:2"),
                error("b.erg", 4, 1, "b 4:1"),
            ],
        );
        problems.report(
            compile("a.erg"),
            vec![
                error("a.erg", 10, 1, "a 10:1"),
                warning("a.erg", 2, 1, "a warning"),
                error("a.erg", 2, 5, "a 2:5"),
            ],
        );

        assert_eq!(
            messages(&problems),
            vec![
                "a 2:5",
                "a 10:1",
                "b 4:1",
                "b 4:2",
                "a warning",
                "b warning"
            ]
        );
    }

    /// **A module's counts are its errors and its warnings**, whichever
    /// source found them — a fault in it counts as surely as a type error.
    #[test]
    fn of_module_counts_the_errors_and_warnings_located_in_it() {
        let problems = Problems::default();
        problems.report(
            compile("guard.erg"),
            vec![
                error("guard.erg", 1, 1, "one"),
                warning("guard.erg", 2, 1, "two"),
                error("helper.erg", 3, 1, "imported"),
            ],
        );
        problems.report(
            ProblemSource::Running,
            vec![error("guard.erg", 0, 0, "faulted")],
        );

        assert_eq!(problems.of_module("guard.erg"), (2, 1));
        assert_eq!(problems.of_module("helper.erg"), (1, 0));
        assert_eq!(problems.of_module("lamp.erg"), (0, 0));
    }

    /// A problem in no module is in no module's count.
    #[test]
    fn a_problem_without_a_module_counts_for_none() {
        let problems = Problems::default();
        let mut orphan = warning("x", 0, 0, "`Health` is declared by no module");
        orphan.module = None;
        problems.report(ProblemSource::Registration, vec![orphan]);

        assert_eq!(problems.all().len(), 1);
        assert_eq!(problems.of_module(""), (0, 0));
        assert_eq!(problems.of_module("x"), (0, 0));
    }

    /// **The generation moves when what is held changes**: a report, a
    /// replacement, a clearing.
    #[test]
    fn the_generation_moves_on_every_change() {
        let problems = Problems::default();
        let start = problems.generation();

        problems.report(compile("guard.erg"), vec![error("guard.erg", 1, 1, "one")]);
        let reported = problems.generation();
        assert_ne!(reported, start, "a first report");

        problems.report(compile("guard.erg"), vec![error("guard.erg", 2, 1, "two")]);
        let replaced = problems.generation();
        assert_ne!(replaced, reported, "a different list");

        problems.report(compile("guard.erg"), Vec::new());
        assert_ne!(problems.generation(), replaced, "a clearing");
    }

    /// **The generation stays when nothing changed.**
    ///
    /// The same list reported again, or a source with nothing clearing again — what a
    /// lane reporting every frame does.
    #[test]
    fn the_generation_stays_when_a_report_changes_nothing() {
        let problems = Problems::default();
        let start = problems.generation();
        problems.report(ProblemSource::Running, Vec::new());
        assert_eq!(
            problems.generation(),
            start,
            "nothing held, nothing cleared"
        );

        problems.report(compile("guard.erg"), vec![error("guard.erg", 1, 1, "one")]);
        let reported = problems.generation();
        problems.report(compile("guard.erg"), vec![error("guard.erg", 1, 1, "one")]);
        assert_eq!(problems.generation(), reported, "the same list again");
    }

    /// A problem in the scene's data: no module, a component and its field.
    fn data_problem(component: &str, field: &str, message: &str) -> Problem {
        Problem {
            component: Some(component.to_owned()),
            field: Some(field.to_owned()),
            ..Problem::new(ProblemSeverity::Error, message)
        }
    }

    /// **The data checks and the running instances each own their list**:
    /// the data source clearing its own leaves the running problem held.
    #[test]
    fn sources_replace_only_their_own_problems() {
        let problems = Problems::default();
        let a = data_problem("Camera", "z_near", "near must be greater than 0 (it is 0)");
        let b = error("guard.erg", 0, 0, "faulted");

        problems.report(ProblemSource::Data, vec![a]);
        problems.report(ProblemSource::Running, vec![b.clone()]);
        problems.report(ProblemSource::Data, Vec::new());

        assert_eq!(problems.all(), vec![b]);
    }

    /// A problem located nowhere names its severity and message, every other field empty.
    #[test]
    fn new_builds_a_problem_located_nowhere() {
        let built = Problem::new(ProblemSeverity::Warning, "it may still land");

        assert_eq!(
            built,
            Problem {
                severity: ProblemSeverity::Warning,
                module: None,
                line: 0,
                column: 0,
                message: "it may still land".to_owned(),
                note: None,
                entity: None,
                behavior: None,
                component: None,
                field: None,
            }
        );
    }

    /// A problem in a file holds the file as its module, and its line and column.
    #[test]
    fn of_file_builds_a_problem_at_a_position_in_a_file() {
        let built = Problem::of_file(ProblemSeverity::Error, "guard.erg", 3, 5, "faulted");

        assert_eq!(
            built,
            Problem {
                severity: ProblemSeverity::Error,
                module: Some("guard.erg".to_owned()),
                line: 3,
                column: 5,
                message: "faulted".to_owned(),
                note: None,
                entity: None,
                behavior: None,
                component: None,
                field: None,
            }
        );
    }

    /// A file problem with no position keeps line and column at 0, what "none" means.
    #[test]
    fn of_file_at_no_position_holds_line_and_column_zero() {
        let built = Problem::of_file(ProblemSeverity::Warning, "shaders/a.wgsl", 0, 0, "ignored");

        assert_eq!((built.line, built.column), (0, 0));
        assert_eq!(built.module.as_deref(), Some("shaders/a.wgsl"));
    }
}
