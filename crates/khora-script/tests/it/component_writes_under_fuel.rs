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

//! A component write whose values are computed — calls, built text, arrays,
//! structs — cut at every slice of fuel: the receiver and the values are
//! evaluated before the write, a stop can fall between them, and the run
//! still queues each write exactly once, with the values a whole run queues.

use khora_core::ecs::entity::EntityId;
use khora_script::vm::{Machine, Program, Run, Value};
use khora_script::{
    compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader, ENGINE_COMPONENTS_MODULE,
};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

const ENGINE: &str = "
component Health {
    int current;
    int max;
}
component Label {
    string text;
    int[] counts;
    Pose pose;
    Vec3? aim;
}
struct Pose {
    Vec3 at;
    string name;
}
";

const MAIN: &str = r#"
fn string Word(int n) {
    int i = 0;
    while (i < n) { i = i + 1; }
    if (n > 2) { return "big"; }
    return "small";
}
fn int Count(int n) {
    int total = 0;
    int i = 0;
    while (i < n) { total = total + i; i = i + 1; }
    return total;
}
fn void Main(Entity e, int n) {
    e.Set(Label { pose: Pose { at: Vec3(1.0, 2.0, 3.0), name: "p" + Word(1) }, counts: [Count(n), Count(2)], text: "size: " + Word(n), aim: null });
    Spawn(Vec3(0.0, 1.0, 0.0), Health { current: Count(3), max: Count(4) }, Label { text: Word(3) + "!" });
    e.Add(Health { max: Count(n) });
}
"#;

fn compile(main: &str) -> CompileOutcome {
    let loader = MemoryLoader::new()
        .with(ENGINE_COMPONENTS_MODULE, ENGINE)
        .with(
            "main.erg",
            format!("import \"{ENGINE_COMPONENTS_MODULE}\";\n{main}"),
        );
    compile_module(&loader, "main.erg")
}

fn build(main: &str) -> Program {
    let result = compile(main);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn args() -> Vec<Value> {
    vec![Value::Entity(SUBJECT), Value::Int(5)]
}

/// **Every slice queues the whole run's writes, once each.** Each cut ends
/// the frame — the arena the values were built in is emptied — and the next
/// frame resumes; the commands of every frame, put together, are the
/// commands of the uninterrupted run, and no run overdraws past the
/// program's bound.
#[test]
fn a_computed_write_cut_at_every_slice_is_queued_once_with_its_values() {
    let program = build(MAIN);

    let mut whole_host = Host::new().for_entity(SUBJECT);
    let mut whole = Machine::new(&program, "Main", &args()).expect("Main exists");
    let (run, cost) = whole.run_counting(&program, &mut whole_host, u64::MAX);
    assert_eq!(run, Run::Completed);
    let expected = whole_host.end_frame().as_slice().to_vec();
    assert_eq!(expected.len(), 3, "a set, a spawn and an add");

    let mut wrong = Vec::new();
    for slice in 1..=cost + 1 {
        let mut host = Host::new().for_entity(SUBJECT);
        let mut machine = Machine::new(&program, "Main", &args()).expect("Main exists");
        let mut queued = Vec::new();
        let mut frames = 0;
        loop {
            let (run, spent) = machine.run_counting(&program, &mut host, slice);
            if spent > slice + program.max_overdraft() {
                wrong.push(format!(
                    "slice {slice}: spent {spent}, past the bound {}",
                    slice + program.max_overdraft()
                ));
            }
            queued.extend(host.end_frame().as_slice().iter().cloned());
            match run {
                Run::Completed => break,
                Run::Suspended(_) => {
                    frames += 1;
                    assert!(frames < 10_000, "slice {slice}: no end");
                }
                Run::Faulted(fault) => {
                    wrong.push(format!("slice {slice}: faulted: {fault:?}"));
                    break;
                }
            }
        }
        if queued != expected {
            wrong.push(format!("slice {slice}: queued {queued:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "expected {expected:?}\n{}",
        wrong.join("\n")
    );
}

/// **A save and a reload between the values and the write.** At every slice,
/// each cut is frozen and resumed into an edited program — one that writes
/// other components *before* `Main`, so every patch and string index `Main`
/// uses moves, whose `Count` body changed, and whose `Main` changed after the
/// writes it stands in — and the commands queued are still those of the
/// uninterrupted run, once each.
#[test]
fn a_computed_write_cut_and_reloaded_into_edited_code_is_queued_once_with_its_values() {
    use khora_core::script::PendingBody;
    use khora_script::vm::{resume, ResumeTier};

    let program = build(MAIN);
    let edited = build(&format!(
        r#"fn void Before(Entity e) {{
               e.Set(Health {{ max: 1, current: 2 }});
               e.Remove(Label);
               Spawn(Vec3(0.0, 0.0, 0.0), Label {{ aim: null, text: "shift" }});
           }}
           {}"#,
        MAIN.replace("int total = 0;", "int total = 0 * n;")
            .replace(
                "e.Add(Health { max: Count(n) });",
                "e.Add(Health { max: Count(n) + 0 });"
            )
    ));

    let mut whole_host = Host::new().for_entity(SUBJECT);
    let mut whole = Machine::new(&program, "Main", &args()).expect("Main exists");
    let (run, cost) = whole.run_counting(&program, &mut whole_host, u64::MAX);
    assert_eq!(run, Run::Completed);
    let expected = whole_host.end_frame().as_slice().to_vec();

    let mut wrong = Vec::new();
    let mut tiers = Vec::new();
    for slice in 1..=cost + 1 {
        let mut host = Host::new().for_entity(SUBJECT);
        let mut machine = Machine::new(&program, "Main", &args()).expect("Main exists");
        let mut current = &program;
        let mut queued = Vec::new();
        let mut restarted = false;
        loop {
            let run = machine.run(current, &mut host, slice);
            queued.extend(host.end_frame().as_slice().iter().cloned());
            match run {
                Run::Completed => break,
                Run::Suspended(_) => {
                    let frozen = machine
                        .freeze(current, PendingBody::Sequence)
                        .expect("a machine of this program freezes");
                    let (next, tier) = resume(&frozen, current.fingerprint(), &edited)
                        .unwrap_or_else(|abandoned| {
                            panic!("slice {slice}: abandoned {abandoned:?}")
                        });
                    restarted |= tier == ResumeTier::Restarted;
                    tiers.push(tier);
                    machine = next;
                    current = &edited;
                }
                Run::Faulted(fault) => {
                    wrong.push(format!("slice {slice}: faulted: {fault:?}"));
                    break;
                }
            }
        }
        if !restarted && queued != expected {
            wrong.push(format!("slice {slice}: queued {queued:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "expected {expected:?}\ntiers {tiers:?}\n{}",
        wrong.join("\n")
    );
}
