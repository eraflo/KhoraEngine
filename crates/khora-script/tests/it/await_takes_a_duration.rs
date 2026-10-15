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

//! `await` takes a duration — a `Duration`, or a `float` of seconds.
//!
//! Anything else is refused by the checker at the line it was written,
//! saying that awaiting an event is not supported yet, instead of compiling
//! into a fault when the body runs.

use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

fn compile(body: &str) -> CompileOutcome {
    let source = format!(
        "behavior Waiter {{
             async void Wait() {{ {body} }}
         }}"
    );
    compile_module(&MemoryLoader::new().with("main.erg", source), "main.erg")
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

fn errors(result: &CompileOutcome) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(said)
        .collect()
}

/// **A duration or a float waits; anything else is refused**, saying what
/// `await` takes and that an event cannot be awaited yet.
#[test]
fn await_of_anything_but_a_duration_is_refused() {
    for waits in [
        "await 0.5s;",
        "Duration d = 1s; await d;",
        "float seconds = 0.25; await seconds;",
    ] {
        let result = compile(waits);
        assert!(
            result.succeeded(),
            "`{waits}` should compile: {:?}",
            errors(&result)
        );
    }

    for refused in [
        "await true;",
        "await \"soon\";",
        "await this;",
        "await Vec3(0.0, 0.0, 0.0);",
        "await 90deg;",
    ] {
        let result = compile(refused);
        assert!(!result.succeeded(), "`{refused}` should be refused");
        let found = errors(&result);
        assert!(
            found
                .iter()
                .any(|text| text.contains("`await` takes a duration")
                    && text.contains("awaiting an event is not supported yet")),
            "`{refused}`: expected the refusal to say what `await` takes; got {found:?}"
        );
    }
}
