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

//! `void FixedUpdate(float dt)` — a member the engine calls once per fixed
//! simulation step.
//!
//! What the compiler holds an author to: the shape `void FixedUpdate(float
//! dt)`, never `async`; written on a behavior, inside a state, or on a
//! system; and never on a behavior that also declares a `[Rate(…)]` — a
//! fixed-step member runs every step, which a rate limit would contradict.
//! The engine calls it, so no warning says otherwise.

use khora_script::{compile_module, CompileOutcome, Diagnostic, MemoryLoader};

fn compile(source: &str) -> CompileOutcome {
    compile_module(&MemoryLoader::new().with("main.erg", source), "main.erg")
}

fn errors(result: &CompileOutcome) -> Vec<&Diagnostic> {
    result.diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

/// Compiles `source` with no error and no warning at all.
fn compiles_cleanly(source: &str) {
    let result = compile(source);
    let found: Vec<String> = result.diagnostics.iter().map(said).collect();
    assert!(
        result.diagnostics.is_empty(),
        "should compile with nothing to say:\n{source}\n{found:?}"
    );
    assert!(result.program.is_some(), "a program with no errors");
}

/// Asserts `source` is refused with an error saying every one of
/// `fragments`, compared without case.
fn refused_saying(source: &str, fragments: &[&str]) {
    let result = compile(source);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{source}"
    );
    let found: Vec<String> = errors(&result).into_iter().map(said).collect();
    assert!(
        found.iter().any(|text| {
            let text = text.to_lowercase();
            fragments
                .iter()
                .all(|fragment| text.contains(&fragment.to_lowercase()))
        }),
        "expected a refusal of\n{source}\nsaying {fragments:?}; got {found:?}"
    );
}

// ─── Called ─────────────────────────────────────────────────────────────────

/// **The engine calls `FixedUpdate`**: the lifecycle table says so, which is
/// what the checker and the dispatcher both read.
#[test]
fn fixed_update_is_a_called_lifecycle_member() {
    let hook = khora_script::lifecycle::of("FixedUpdate").expect("`FixedUpdate` is listed");
    assert!(std::ptr::eq(hook, &khora_script::lifecycle::FIXED_UPDATE));
    assert!(
        khora_script::lifecycle::FIXED_UPDATE.called,
        "the engine calls `FixedUpdate`"
    );
}

// ─── Its shape ──────────────────────────────────────────────────────────────

/// **`void FixedUpdate(float dt)` compiles with no warning** — nothing says it
/// is reserved any more — and any other shape is refused as `Update`'s is.
#[test]
fn fixed_update_takes_the_step_and_compiles_without_a_warning() {
    compiles_cleanly(
        "behavior Mover {
             float x = 0.0;
             void FixedUpdate(float dt) { x += dt; }
             void Update(float dt) { x += 0.0; }
         }",
    );

    refused_saying(
        "behavior Mover { void FixedUpdate() { } }",
        &["`FixedUpdate` must take (float)"],
    );
    refused_saying(
        "behavior Mover { void FixedUpdate(int steps) { } }",
        &["`FixedUpdate` must take (float)"],
    );
    refused_saying(
        "behavior Mover { float FixedUpdate(float dt) { return dt; } }",
        &["`FixedUpdate` must return nothing"],
    );
}

/// **`FixedUpdate` is not `async`**: each step is one call, so it may not
/// wait for time to pass.
#[test]
fn an_async_fixed_update_is_refused() {
    refused_saying(
        "behavior Mover { async void FixedUpdate(float dt) { await 1s; } }",
        &["async", "FixedUpdate"],
    );
    refused_saying(
        "behavior Mover { async void FixedUpdate(float dt) { } }",
        &["async", "FixedUpdate"],
    );
}

// ─── Where it is written ────────────────────────────────────────────────────

/// **A state may declare its own `FixedUpdate`**, as it may its own
/// `Update` — with no warning either.
#[test]
fn fixed_update_compiles_inside_a_state() {
    compiles_cleanly(
        "behavior Mover {
             float x = 0.0;
             state Walking {
                 void FixedUpdate(float dt) { x += dt; }
             }
             state Resting { }
         }",
    );
}

/// **A system may declare `FixedUpdate`**, with no warning.
#[test]
fn fixed_update_compiles_on_a_system() {
    compiles_cleanly(
        "system Integrator {
             float total = 0.0;
             void FixedUpdate(float dt) { total += dt; }
         }",
    );
}

// ─── Not with a rate ────────────────────────────────────────────────────────

/// **A rate-limited behavior has no `FixedUpdate`**: a fixed-step member runs
/// every step, which `[Rate(…)]` would contradict — refused whether it is
/// written on the behavior, inside one of its states, or on a system.
#[test]
fn a_rate_limited_behavior_declaring_fixed_update_is_refused() {
    for source in [
        "[Rate(10hz)]
         behavior Mover {
             float x = 0.0;
             void FixedUpdate(float dt) { x += dt; }
         }",
        "[Rate(10hz)]
         behavior Mover {
             float x = 0.0;
             state Walking { void FixedUpdate(float dt) { x += dt; } }
         }",
        "[Rate(10hz)]
         system Integrator {
             float total = 0.0;
             void FixedUpdate(float dt) { total += dt; }
         }",
    ] {
        refused_saying(source, &["fixed-step", "every step"]);
    }

    // The rate alone, and `FixedUpdate` alone, are each fine.
    compiles_cleanly(
        "[Rate(10hz)]
         behavior Mover { float x = 0.0; void Update(float dt) { x += dt; } }",
    );
}
