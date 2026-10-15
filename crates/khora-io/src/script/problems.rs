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
//! [`ScriptProblems`] — located through the program's source map, each
//! problem naming the module it is in.

use khora_core::script::{ProblemSeverity, ProblemSource, ScriptProblem, ScriptProblems};
use khora_script::diagnostics::Severity;
use khora_script::CompileOutcome;

/// Reports what compiling `module` found, replacing what its last compile
/// reported — none clears it. Each problem names the module it is located in,
/// which may be one `module` imports: compiling that module may find it too,
/// and the list shows it once; whichever compile found it is the one whose
/// clean compile takes it back.
pub fn report_compile(problems: &ScriptProblems, module: &str, outcome: &CompileOutcome) {
    let found = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let location = outcome.sources.locate(diagnostic.span);
            let (owner, line, column) = match location {
                Some(location) => (location.module, location.line, location.column),
                None => (module.to_owned(), 0, 0),
            };
            ScriptProblem {
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
