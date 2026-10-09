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

//! Arrays at run time: literals, indexing, `.Length`, `foreach`, zeros, and
//! what binding one to a name means.
//!
//! An array is a value (spec 05, D1): binding one read from a place — a local,
//! a field, an element — to a name copies it, so writing through the new name
//! never changes the old one. Writing through a path (`grid[0][1] = 7`)
//! changes the array the path names, in place.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "ArrayTextTail")]
fn array_text_tail() -> String {
    "bcd".to_owned()
}

/// The entity a behavior's instance belongs to.
const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// Compiles `source`, which must check and compile without an error.
fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

/// Runs `machine` to completion on `host`, an `await` treated as elapsed.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end");
}

/// Runs the free function `function` of `program` with `args`.
fn run(program: &Program, function: &str, args: &[Value]) -> Result<Value, Fault> {
    let machine = Machine::new(program, function, args)
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    finish(machine, program, &mut Host::new())
}

/// The text `function` returns.
fn run_text(program: &Program, function: &str) -> Result<String, String> {
    let mut machine = Machine::new(program, function, &[])
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    let mut host = Host::new();
    match machine.run(program, &mut host, u64::MAX) {
        Run::Completed => machine
            .resolve_str(machine.result(), program, &host)
            .map(str::to_owned)
            .map_err(|fault| format!("{fault:?}")),
        other => Err(format!("{other:?}")),
    }
}

/// A fresh instance of `behavior`, created as the engine creates one: fields
/// sized from the layout, declared defaults run.
fn instance(program: &Program, behavior: &str) -> Host {
    let layout = program
        .layout(behavior)
        .unwrap_or_else(|| panic!("`{behavior}` is a behavior of the program"));
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name(behavior), &[]).expect("a field initialiser");
    finish(init, program, &mut host).expect("the initialiser runs");
    host
}

/// Runs `member` with `args` on the instance `host` holds.
fn call(
    program: &Program,
    behavior: &str,
    member: &str,
    args: &[Value],
    host: &mut Host,
) -> Result<Value, Fault> {
    let function = resolve_member(program, behavior, member, host)
        .unwrap_or_else(|| panic!("`{behavior}` has a member `{member}`"));
    let machine = Machine::new(program, &function, args).expect("the member exists");
    finish(machine, program, host)
}

/// Runs every `(function, expected)` of `program`, naming each that differs.
fn expect_all(program: &Program, cases: &[(&str, Value)]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(function, wanted)| {
            let got = run(program, function, &[]);
            (got.as_ref() != Ok(wanted))
                .then(|| format!("`{function}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `count` integer literals `0, 1, …`, comma-separated.
fn ints(count: usize) -> String {
    (0..count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// ─── Indexing ───────────────────────────────────────────────────────────────

/// **An index outside the array faults, naming the index and the length** —
/// past the end, negative, on an empty array, and on a write. Never a panic,
/// never another element.
#[test]
fn an_index_out_of_range_faults() {
    let program = build(
        "fn int Past() { return [1][5]; }
         fn int Negative() { int i = -1; return [1][i]; }
         fn int Empty() { int[] xs = []; return xs[0]; }
         fn int Write() { int[] xs = [1, 2]; xs[2] = 0; return 0; }",
    );
    let cases = [
        ("Past", Fault::IndexOutOfRange { index: 5, len: 1 }),
        ("Negative", Fault::IndexOutOfRange { index: -1, len: 1 }),
        ("Empty", Fault::IndexOutOfRange { index: 0, len: 0 }),
        ("Write", Fault::IndexOutOfRange { index: 2, len: 2 }),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(function, wanted)| {
            let got = run(&program, function, &[]);
            (got.as_ref() != Err(wanted))
                .then(|| format!("`{function}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Elements read back as what was put in them, whatever their type — text
/// built while running included — and an element written reads back as the
/// new value.
#[test]
fn elements_read_back_as_written() {
    let program = build(
        r#"fn int Ints() { int[] xs = [4, 5, 6]; return xs[0] * 100 + xs[1] * 10 + xs[2]; }
           fn float Floats() { float[] xs = [0.5, 1.5]; return xs[1]; }
           fn bool Bools() { bool[] xs = [false, true]; return xs[1]; }
           fn int Written() { int[] xs = [1, 2, 3]; xs[1] = 8; return xs[1]; }
           fn int Compound() { int[] xs = [1, 2, 3]; xs[2] += 4; return xs[2]; }
           fn int Nested() { int[][] grid = [[1, 2], [3]]; grid[0][1] = 7; return grid[0][1] * 10 + grid[1][0]; }
           fn int Indexed(int i) { int[] xs = [10, 20, 30]; return xs[i]; }"#,
    );
    expect_all(
        &program,
        &[
            ("Ints", Value::Int(456)),
            ("Floats", Value::Float(1.5)),
            ("Bools", Value::Bool(true)),
            ("Written", Value::Int(8)),
            ("Compound", Value::Int(7)),
            ("Nested", Value::Int(73)),
        ],
    );
    assert_eq!(
        run(&program, "Indexed", &[Value::Int(2)]),
        Ok(Value::Int(30))
    );

    let text = build(
        r#"fn string Built() { string[] names = ["a", "b" + ArrayTextTail()]; return names[1] + names[0]; }"#,
    );
    assert_eq!(run_text(&text, "Built"), Ok("bbcda".to_owned()));
}

// ─── `.Length` ──────────────────────────────────────────────────────────────

/// `.Length` counts an array's elements and a string's characters — a literal
/// or text built while running.
#[test]
fn length_counts_elements_and_characters() {
    let program = build(
        r#"fn int Array() { int[] xs = [1, 2, 3, 4]; return xs.Length; }
           fn int Empty() { int[] xs = []; return xs.Length; }
           fn int Literal() { return "héllo".Length; }
           fn int Built() { string s = "ab" + ArrayTextTail(); return s.Length; }
           fn int Element() { string[] names = ["abc", "de"]; return names[1].Length; }"#,
    );
    expect_all(
        &program,
        &[
            ("Array", Value::Int(4)),
            ("Empty", Value::Int(0)),
            ("Literal", Value::Int(5)),
            ("Built", Value::Int(5)),
            ("Element", Value::Int(2)),
        ],
    );
}

// ─── Zeros ──────────────────────────────────────────────────────────────────

/// **`T[]` has a zero: `[]`.** A local declared without a value, and a field
/// or a state datum declared without a default, start empty.
#[test]
fn an_array_without_a_value_starts_empty() {
    let program = build("fn int F() { int[] xs; return xs.Length; }");
    assert_eq!(run(&program, "F", &[]), Ok(Value::Int(0)));

    let program = build(
        "behavior Guard {
             string[] names;
             state Patrol {
                 int[] route;
                 int Route() { return route.Length; }
             }
             int Names() { return names.Length; }
         }",
    );
    let mut host = instance(&program, "Guard");
    assert_eq!(
        call(&program, "Guard", "Names", &[], &mut host),
        Ok(Value::Int(0))
    );
    assert_eq!(
        call(&program, "Guard", "Route", &[], &mut host),
        Ok(Value::Int(0))
    );
}

// ─── Fields ─────────────────────────────────────────────────────────────────

/// **The bug spec 05 names: a field load always made a string.** An array in a
/// behavior field reads back as that array — its elements and its length — in
/// a later frame, after the arena it was built in was emptied; and a write
/// through the field is still there in the frame after.
#[test]
fn an_array_in_a_field_loads_as_an_array() {
    let program = build(
        r#"behavior Guard {
               int[] xs = [1, 2, 3];
               string[] names = ["a", "b" + ArrayTextTail()];
               int Second() { return xs[1]; }
               int Count() { return xs.Length; }
               int NameLength() { return names[1].Length; }
               void Write() { xs[1] = 5; }
               void Replace() { xs = [7, 8, 9, 10]; }
           }"#,
    );
    let mut host = instance(&program, "Guard");
    host.end_frame();

    assert_eq!(
        call(&program, "Guard", "Second", &[], &mut host),
        Ok(Value::Int(2))
    );
    assert_eq!(
        call(&program, "Guard", "Count", &[], &mut host),
        Ok(Value::Int(3))
    );
    assert_eq!(
        call(&program, "Guard", "NameLength", &[], &mut host),
        Ok(Value::Int(4)),
        "a string inside an array field is still its text"
    );

    call(&program, "Guard", "Write", &[], &mut host).expect("the write runs");
    host.end_frame();
    assert_eq!(
        call(&program, "Guard", "Second", &[], &mut host),
        Ok(Value::Int(5)),
        "a write through the field lands in the field"
    );

    call(&program, "Guard", "Replace", &[], &mut host).expect("the assignment runs");
    host.end_frame();
    assert_eq!(
        call(&program, "Guard", "Count", &[], &mut host),
        Ok(Value::Int(4)),
        "an array assigned to the field replaces it"
    );
}

// ─── Value semantics ────────────────────────────────────────────────────────

/// **D1.** Binding an array read from a place to a name copies it: a `let`, an
/// assignment, an argument, a `foreach` element, a ternary's result, an
/// element bound whole, a field read into a local. Writing through the new
/// name leaves the original as it was.
#[test]
fn arrays_have_value_semantics() {
    let program = build(
        "fn int Let() { int[] a = [1, 2, 3]; int[] b = a; b[0] = 9; return a[0] * 10 + b[0]; }
         fn int Assign() { int[] a = [1, 2]; int[] b = [0]; b = a; b[1] = 9; return a[1] * 10 + b[1]; }
         fn int Poke(int[] xs) { xs[0] = 9; return xs[0]; }
         fn int Argument() { int[] a = [1, 2]; int seen = Poke(a); return a[0] * 10 + seen; }
         fn int Element() {
             int[][] grid = [[1], [2]];
             foreach (var row in grid) { row[0] = 9; }
             return grid[0][0] * 10 + grid[1][0];
         }
         fn int Ternary(bool pick) { int[] a = [1]; int[] b = pick ? a : a; b[0] = 9; return a[0]; }
         fn int Row() { int[][] grid = [[1, 2]]; int[] row = grid[0]; row[0] = 9; return grid[0][0]; }
         fn int Back() { int[] a = [1]; int[] b = a; a[0] = 5; return b[0]; }",
    );
    expect_all(
        &program,
        &[
            ("Let", Value::Int(19)),
            ("Assign", Value::Int(29)),
            ("Argument", Value::Int(19)),
            ("Element", Value::Int(12)),
            ("Row", Value::Int(1)),
            ("Back", Value::Int(1)),
        ],
    );
    assert_eq!(
        run(&program, "Ternary", &[Value::Bool(true)]),
        Ok(Value::Int(1)),
        "a ternary's result read from a place is a copy"
    );

    // A field read into a local is a copy too: writing the local leaves the
    // field alone.
    let program = build(
        "behavior Guard {
             int[] route = [1, 2];
             int Probe() { int[] mine = route; mine[0] = 9; return route[0]; }
             int First() { return route[0]; }
         }",
    );
    let mut host = instance(&program, "Guard");
    assert_eq!(
        call(&program, "Guard", "Probe", &[], &mut host),
        Ok(Value::Int(1))
    );
    host.end_frame();
    assert_eq!(
        call(&program, "Guard", "First", &[], &mut host),
        Ok(Value::Int(1)),
        "the field still holds its own value"
    );
}

// ─── `foreach` ──────────────────────────────────────────────────────────────

/// `foreach` visits every element in order, with `break` and `continue`, a
/// written element type, an empty array, a nested loop, and an array from a
/// field.
#[test]
fn foreach_visits_every_element_in_order() {
    let program = build(
        "fn int Order() { int n = 0; foreach (var x in [1, 2, 3]) { n = n * 10 + x; } return n; }
         fn int Typed() { int sum = 0; foreach (int x in [4, 5]) { sum = sum + x; } return sum; }
         fn int Empty() { int[] xs = []; int n = 0; foreach (var x in xs) { n = n + 1; } return n; }
         fn int Skips() {
             int sum = 0;
             foreach (var x in [1, 2, 3, 4, 5]) {
                 if (x == 2) { continue; }
                 if (x == 5) { break; }
                 sum = sum + x;
             }
             return sum;
         }
         fn int Nested() {
             int n = 0;
             foreach (var row in [[1, 2], [3]]) { foreach (var x in row) { n = n * 10 + x; } }
             return n;
         }",
    );
    expect_all(
        &program,
        &[
            ("Order", Value::Int(123)),
            ("Typed", Value::Int(9)),
            ("Empty", Value::Int(0)),
            ("Skips", Value::Int(8)),
            ("Nested", Value::Int(123)),
        ],
    );

    let program = build(
        "behavior Guard {
             int[] route = [3, 4];
             int Sum() { int sum = 0; foreach (var x in route) { sum = sum + x; } return sum; }
         }",
    );
    let mut host = instance(&program, "Guard");
    host.end_frame();
    assert_eq!(
        call(&program, "Guard", "Sum", &[], &mut host),
        Ok(Value::Int(7))
    );
}

/// **`foreach` iterates a snapshot.** Writing to the array being walked, from
/// inside the loop, changes the array but not what the loop visits.
#[test]
fn foreach_iterates_a_snapshot() {
    let program = build(
        "fn int F() {
             int[] xs = [1, 2, 3];
             int sum = 0;
             foreach (var x in xs) { xs[2] = 100; sum = sum + x; }
             return sum * 1000 + xs[2];
         }",
    );
    assert_eq!(run(&program, "F", &[]), Ok(Value::Int(6100)));

    let program = build(
        "behavior Guard {
             int[] route = [1, 2, 3];
             int Walk() {
                 int sum = 0;
                 foreach (var x in route) { route = [50]; sum = sum + x; }
                 return sum * 100 + route[0];
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    assert_eq!(
        call(&program, "Guard", "Walk", &[], &mut host),
        Ok(Value::Int(650))
    );
}

// ─── Long literals and registers ────────────────────────────────────────────

/// **A literal longer than one batch of registers builds in chunks.** Three
/// hundred elements — more than a register index can name — compile and read
/// back, first, middle and last; so does one just past a batch.
#[test]
fn a_long_literal_builds_in_chunks() {
    let program = build(&format!(
        "fn int Long() {{ int[] xs = [{}]; return xs[0] + xs[150] + xs[299] + xs.Length; }}
         fn int JustPast() {{ int[] xs = [{}]; return xs[16] * 100 + xs.Length; }}",
        ints(300),
        ints(17)
    ));
    expect_all(
        &program,
        &[("Long", Value::Int(749)), ("JustPast", Value::Int(1617))],
    );
}

/// **A function needing more registers than a frame can name is refused** by
/// the compiler, never compiled into registers that wrap around onto each
/// other.
#[test]
fn a_function_needing_more_than_256_registers_is_refused() {
    let locals: String = (0..300).map(|n| format!("int v{n} = {n}; ")).collect();
    let source = format!("fn int F() {{ {locals} return v0 + v299; }}");
    let lexed = lex(&source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);

    let compiled = compile(&parsed.module);
    assert!(
        compiled.has_errors()
            && compiled
                .diagnostics
                .iter()
                .any(|d| d.is_error() && d.message.contains("register")),
        "expected a compile error naming the registers, got {:?}",
        compiled.diagnostics
    );
}

// ─── `become` ───────────────────────────────────────────────────────────────

/// **A state entered with an array holds its own copy of it**, in the durable
/// store: the state reads it in a later frame, and the array the caller still
/// holds is not the state's.
#[test]
fn a_state_entered_with_an_array_holds_it() {
    let program = build(
        "behavior Guard {
             state Patrol {
                 int Spot() {
                     int[] route = [4, 5];
                     become Chase(route);
                     route[0] = 9;
                     return route[0];
                 }
             }
             state Chase(int[] path) {
                 int First() { return path[0] * 10 + path.Length; }
             }
         }",
    );
    let mut host = instance(&program, "Guard");
    assert_eq!(
        call(&program, "Guard", "Spot", &[], &mut host),
        Ok(Value::Int(9))
    );
    host.end_frame();
    assert_eq!(
        call(&program, "Guard", "First", &[], &mut host),
        Ok(Value::Int(42)),
        "the state's datum is the array as it was entered with"
    );
}
