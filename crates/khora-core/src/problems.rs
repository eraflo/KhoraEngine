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
//! modules declare, a declaration deleted), the script lane (an instance that
//! faulted, a body that runs too long), and the scan of an assets root for
//! `.erg` files that sit outside its `scripts/` folder and so are never
//! compiled. For the scene's data: the components' own checks (a camera whose
//! near plane sits at its eye), and what the last scene load had to adapt.
//! Each reports its own list here, already located, and replaces it each time
//! it reports.

use crate::ecs::entity::EntityId;

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

/// Every source's problems. Shared as an `Arc` in the runtime's resources.
#[derive(Debug, Default)]
pub struct Problems(std::sync::RwLock<Held>);

/// What is held: each source's list, and how many changes there were.
#[derive(Debug, Default)]
struct Held {
    by_source: std::collections::HashMap<ProblemSource, Vec<Problem>>,
    generation: u64,
}

impl Problems {
    /// Replaces what `source` reported last with `problems`; an empty list
    /// clears it.
    pub fn report(&self, source: ProblemSource, problems: Vec<Problem>) {
        let mut held = match self.0.write() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
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

    /// Every problem of every source: errors first, then by module, line,
    /// column, component and field — each once, though two compiles may both
    /// find one in a module they share.
    pub fn all(&self) -> Vec<Problem> {
        let held = match self.0.read() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
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

    /// What `source` reported last, as it reported it; empty when it holds
    /// nothing.
    pub fn of_source(&self, source: &ProblemSource) -> Vec<Problem> {
        let held = match self.0.read() {
            Ok(held) => held,
            Err(poisoned) => poisoned.into_inner(),
        };
        held.by_source.get(source).cloned().unwrap_or_default()
    }

    /// How many errors and warnings are located in `module`.
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
        match self.0.read() {
            Ok(held) => held.generation,
            Err(poisoned) => poisoned.into_inner().generation,
        }
    }
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
        Problem {
            severity,
            module: Some(module.to_owned()),
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

    /// **The generation stays when nothing changed**: the same list reported
    /// again, or a source with nothing clearing again — what a lane reporting
    /// every frame does.
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
            severity: ProblemSeverity::Error,
            module: None,
            line: 0,
            column: 0,
            message: message.to_owned(),
            note: None,
            entity: None,
            behavior: None,
            component: Some(component.to_owned()),
            field: Some(field.to_owned()),
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
}
