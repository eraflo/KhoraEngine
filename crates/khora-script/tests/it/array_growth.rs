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

//! Growing and shrinking an array: `xs.Push(v)` and `xs.RemoveAt(i)` (spec
//! 05, D1b).
//!
//! Both work on any place — a local, a behavior field, an element, a struct
//! field, any path of those — and change the array the place names, in place.
//! A path rooted at a behavior field is the field loaded, changed and written
//! back. `Push` keeps its own copy of an object it is given (value
//! semantics); `RemoveAt` shifts the elements after the index down by one,
//! and an index outside the array faults, naming the index and the length.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::{Object, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Suspension, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "ArrayGrowthTail")]
fn array_growth_tail() -> String {
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

/// Puts other objects at the first indices of the arena.
fn occupy(host: &mut Host) {
    for n in 0..4 {
        host.arena
            .alloc(Object::Array(vec![Value::Int(-1); n + 1]))
            .expect("the arena has room");
        host.arena
            .alloc(Object::Str("other".to_owned()))
            .expect("the arena has room");
    }
}

/// Ends the frame as the lane does and fills the next one with other objects.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

/// Runs `machine` to completion on `host`, a frame per suspension.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => next_frame(host),
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

/// A fresh instance of `Guard`, created as the engine creates one.
fn instance(program: &Program) -> Host {
    let layout = program.layout("Guard").expect("the guard's layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(program, &init_name("Guard"), &[]).expect("a field initialiser");
    finish(init, program, &mut host).expect("the initialiser runs");
    host.end_frame();
    host
}

fn machine_for(program: &Program, member: &str, args: &[Value], host: &Host) -> Machine {
    let function = resolve_member(program, "Guard", member, host)
        .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
    Machine::new(program, &function, args).expect("the member exists")
}

/// Runs `member` with `args` on the guard `host` holds, then ends the frame.
fn call(program: &Program, member: &str, args: &[Value], host: &mut Host) -> Result<Value, Fault> {
    let machine = machine_for(program, member, args, host);
    let result = finish(machine, program, host);
    next_frame(host);
    result
}

/// Runs every `(function, expected)` of `program`, naming each that differs.
fn expect_all(program: &Program, cases: &[(&str, Result<Value, Fault>)]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(function, wanted)| {
            let got = run(program, function, &[]);
            (got != *wanted).then(|| format!("`{function}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── On a local and through a path ──────────────────────────────────────────

/// **`Push` appends, `RemoveAt` shifts the rest down** — on a local, from
/// empty and down to empty, at the last index, on an element (by a literal
/// index and a computed one), on a struct field, on a path several steps
/// deep, on an array of structs, on a parameter (the caller's array left
/// alone), in a loop and a `foreach`, and on an array of text built running.
#[test]
fn push_appends_and_remove_at_shifts() {
    let program = build(
        "struct Loot { int value; }
         struct Bag { int[] items; }
         struct Shelf { Bag[] bags; }
         fn int Local() {
             int[] xs = [1, 2];
             xs.Push(3);
             xs.Push(4);
             xs.RemoveAt(0);
             xs.RemoveAt(1);
             xs.Push(5);
             return xs.Length * 1000 + xs[0] * 100 + xs[1] * 10 + xs[2];
         }
         fn int FromEmpty() { int[] xs = []; xs.Push(7); return xs.Length * 10 + xs[0]; }
         fn int ToEmpty() { int[] xs = [7]; xs.RemoveAt(0); return xs.Length; }
         fn int Last() { int[] xs = [1, 2, 3]; xs.RemoveAt(2); return xs.Length * 10 + xs[1]; }
         fn int Element() {
             int[][] grid = [[1], [2]];
             grid[0].Push(3);
             grid[1].RemoveAt(0);
             return grid[0].Length * 100 + grid[0][1] * 10 + grid[1].Length;
         }
         fn int Indexed() { int[][] grid = [[1], [2]]; int i = 1; grid[i].Push(5); return grid[1][1] * 10 + grid[0].Length; }
         fn int StructField() {
             Bag bag = Bag { };
             bag.items.Push(4);
             bag.items.Push(5);
             bag.items.RemoveAt(0);
             return bag.items.Length * 10 + bag.items[0];
         }
         fn int Nested() {
             Shelf shelf = Shelf { bags: [Bag { }, Bag { items: [1] }] };
             shelf.bags[1].items.Push(6);
             shelf.bags[0].items.Push(2);
             shelf.bags.Push(Bag { items: [9] });
             shelf.bags.RemoveAt(0);
             return shelf.bags.Length * 1000 + shelf.bags[0].items[1] * 100
                 + shelf.bags[0].items.Length * 10 + shelf.bags[1].items[0];
         }
         fn int Structs() {
             Loot[] items = [];
             items.Push(Loot { value: 3 });
             items.Push(Loot { value: 4 });
             items.RemoveAt(0);
             return items.Length * 10 + items[0].value;
         }
         fn int Grow(int[] xs) { xs.Push(9); return xs.Length; }
         fn int Argument() { int[] a = [1]; int n = Grow(a); return a.Length * 10 + n; }
         fn int InLoop() {
             int[] xs = [];
             int i = 0;
             while (i < 5) { xs.Push(i * i); i += 1; }
             xs.RemoveAt(0);
             return xs.Length * 100 + xs[3];
         }
         fn int InForeach() {
             int[] seen = [];
             foreach (var x in [3, 1, 2]) { seen.Push(x * 10); }
             return seen[0] + seen[1] + seen[2] + seen.Length;
         }",
    );
    expect_all(
        &program,
        &[
            ("Local", Ok(Value::Int(3245))),
            ("FromEmpty", Ok(Value::Int(17))),
            ("ToEmpty", Ok(Value::Int(0))),
            ("Last", Ok(Value::Int(22))),
            ("Element", Ok(Value::Int(230))),
            ("Indexed", Ok(Value::Int(51))),
            ("StructField", Ok(Value::Int(15))),
            ("Nested", Ok(Value::Int(2629))),
            ("Structs", Ok(Value::Int(14))),
            ("Argument", Ok(Value::Int(12))),
            ("InLoop", Ok(Value::Int(416))),
            ("InForeach", Ok(Value::Int(63))),
        ],
    );

    let text = build(
        r#"fn string Names() {
               string[] names = [];
               names.Push("a");
               names.Push("b" + ArrayGrowthTail());
               names.Push("c");
               names.RemoveAt(0);
               return names[0] + names[1];
           }"#,
    );
    assert_eq!(run_text(&text, "Names"), Ok("bbcdc".to_owned()));
}

// ─── On a behavior field ────────────────────────────────────────────────────

const GUARD: &str = r#"struct Loot { int value; }
                       struct Bag { Loot[] items; int count; }
                       behavior Guard {
                           int[] ids = [1, 2];
                           int[][] grid = [[1], [2]];
                           Bag bag = Bag { count: 0 };
                           string[] names = [];
                           state Patrol {
                               int[] route;
                               void Walk(int v) { route.Push(v); }
                               int Route() { return route.Length * 10 + route[route.Length - 1]; }
                           }
                           void Append(int v) { ids.Push(v); }
                           void Drop(int i) { ids.RemoveAt(i); }
                           void Grow(int v) { grid[1].Push(v); }
                           void Pack(int v) { bag.items.Push(Loot { value: v }); bag.count += 1; }
                           void Unpack() { bag.items.RemoveAt(0); }
                           void Name() { names.Push("b" + ArrayGrowthTail()); }
                           int Ids() { return ids.Length * 1000 + ids[0] * 100 + ids[1] * 10 + ids[ids.Length - 1]; }
                           int Grid() { return grid[1].Length * 100 + grid[1][1] * 10 + grid[0].Length; }
                           int Bagged() { return bag.items.Length * 100 + bag.items[0].value * 10 + bag.count; }
                           int Named() { return names.Length * 10 + names[0].Length; }
                       }"#;

/// **A field grows and shrinks across frames.** `Push` and `RemoveAt` on a
/// field, an element of a field, a struct field of a field and a state's
/// datum each land in the field: read in a later frame, after the arena they
/// ran in was emptied and filled with other objects, the field holds what
/// they made of it.
#[test]
fn a_field_grown_and_shrunk_keeps_what_was_done_to_it() {
    let program = build(GUARD);
    let mut host = instance(&program);
    let steps: [(&str, &[Value]); 9] = [
        ("Append", &[Value::Int(3)]),
        ("Append", &[Value::Int(4)]),
        ("Drop", &[Value::Int(0)]),
        ("Grow", &[Value::Int(5)]),
        ("Pack", &[Value::Int(3)]),
        ("Pack", &[Value::Int(4)]),
        ("Unpack", &[]),
        ("Walk", &[Value::Int(6)]),
        ("Walk", &[Value::Int(8)]),
    ];
    for (member, args) in steps {
        call(&program, member, args, &mut host)
            .unwrap_or_else(|fault| panic!("`{member}{args:?}` faulted: {fault:?}"));
    }
    call(&program, "Name", &[], &mut host).expect("`Name` runs");

    let reads = [
        ("Ids", Value::Int(3234)),
        ("Grid", Value::Int(251)),
        ("Bagged", Value::Int(142)),
        ("Route", Value::Int(28)),
        ("Named", Value::Int(14)),
    ];
    let wrong: Vec<String> = reads
        .iter()
        .filter_map(|(member, wanted)| {
            let got = call(&program, member, &[], &mut host);
            (got.as_ref() != Ok(wanted))
                .then(|| format!("`{member}`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ─── Value semantics ────────────────────────────────────────────────────────

/// **`Push` keeps its own copy of an object it is given** (D1). Writing the
/// local that was pushed leaves the pushed element alone, and writing the
/// element leaves the local alone — for an array and a struct, pushed twice,
/// an element of the array pushed onto that same array, a field pushed onto
/// a local, and a local pushed onto a field.
#[test]
fn push_copies_an_object_it_is_given() {
    let program = build(
        "struct Loot { int value; }
         fn int Row() { int[][] grid = []; int[] row = [1]; grid.Push(row); row[0] = 9; return grid[0][0] * 10 + row[0]; }
         fn int Back() { int[][] grid = []; int[] row = [1]; grid.Push(row); grid[0][0] = 9; return row[0]; }
         fn int Twice() { int[][] grid = []; int[] row = [1]; grid.Push(row); grid.Push(row); grid[0][0] = 9; return grid[1][0]; }
         fn int Itself() { int[][] grid = [[1]]; grid.Push(grid[0]); grid[0][0] = 9; return grid[1][0] * 10 + grid.Length; }
         fn int Struct() { Loot[] items = []; Loot l = Loot { value: 1 }; items.Push(l); l.value = 9; return items[0].value; }
         fn int StructBack() { Loot[] items = []; Loot l = Loot { value: 1 }; items.Push(l); items[0].value = 9; return l.value; }",
    );
    expect_all(
        &program,
        &[
            ("Row", Ok(Value::Int(19))),
            ("Back", Ok(Value::Int(1))),
            ("Twice", Ok(Value::Int(1))),
            ("Itself", Ok(Value::Int(12))),
            ("Struct", Ok(Value::Int(1))),
            ("StructBack", Ok(Value::Int(1))),
        ],
    );

    let program = build(
        "behavior Guard {
             int[] route = [1, 2];
             int[][] rows = [];
             int Probe() { int[][] all = []; all.Push(route); all[0][0] = 9; return route[0]; }
             void Keep() { int[] row = [5]; rows.Push(row); row[0] = 9; }
             int First() { return rows[0][0] * 10 + route[0]; }
         }",
    );
    let mut host = instance(&program);
    assert_eq!(
        call(&program, "Probe", &[], &mut host),
        Ok(Value::Int(1)),
        "a field pushed onto a local is a copy of the field"
    );
    call(&program, "Keep", &[], &mut host).expect("`Keep` runs");
    assert_eq!(
        call(&program, "First", &[], &mut host),
        Ok(Value::Int(51)),
        "the field keeps the row as it was pushed, and `route` as it was"
    );
}

// ─── Faults ─────────────────────────────────────────────────────────────────

/// **`RemoveAt` outside the array faults, naming the index and the length** —
/// past the end, at the length, negative, on an empty array, on an element.
/// A `Push` through an element that does not exist faults the same way. On a
/// field, the fault leaves the field as it was.
#[test]
fn remove_at_out_of_range_faults() {
    let program = build(
        "fn int Past() { int[] xs = [1]; xs.RemoveAt(5); return 0; }
         fn int AtLength() { int[] xs = [1, 2]; xs.RemoveAt(2); return 0; }
         fn int Negative() { int[] xs = [1]; int i = -1; xs.RemoveAt(i); return 0; }
         fn int Empty() { int[] xs = []; xs.RemoveAt(0); return 0; }
         fn int Element() { int[][] grid = [[1]]; grid[0].RemoveAt(1); return 0; }
         fn int PushThrough() { int[][] grid = [[1]]; grid[3].Push(1); return 0; }",
    );
    expect_all(
        &program,
        &[
            ("Past", Err(Fault::IndexOutOfRange { index: 5, len: 1 })),
            ("AtLength", Err(Fault::IndexOutOfRange { index: 2, len: 2 })),
            (
                "Negative",
                Err(Fault::IndexOutOfRange { index: -1, len: 1 }),
            ),
            ("Empty", Err(Fault::IndexOutOfRange { index: 0, len: 0 })),
            ("Element", Err(Fault::IndexOutOfRange { index: 1, len: 1 })),
            (
                "PushThrough",
                Err(Fault::IndexOutOfRange { index: 3, len: 1 }),
            ),
        ],
    );

    let program = build(GUARD);
    let mut host = instance(&program);
    assert_eq!(
        call(&program, "Drop", &[Value::Int(5)], &mut host),
        Err(Fault::IndexOutOfRange { index: 5, len: 2 })
    );
    assert_eq!(
        call(&program, "Ids", &[], &mut host),
        Ok(Value::Int(2122)),
        "the faulted removal left the field as it was"
    );
}

// ─── Arguments that wait ────────────────────────────────────────────────────

const WAITER: &str = r#"behavior Guard {
                            int[] ids = [1, 2, 3];
                            async int Slow() { await 1.0s; return 7; }
                            async int Which() { await 1.0s; return 1; }
                            async void Grow() { ids.Push(Slow()); }
                            async void Shrink() { ids.RemoveAt(Which()); }
                            async int Local() {
                                int[] ys = [1, 2];
                                ys.Push(Slow());
                                ys.RemoveAt(Which());
                                return ys.Length * 100 + ys[0] * 10 + ys[1];
                            }
                            void Poke() { ids[0] = 5; }
                            int Count() { return ids.Length; }
                            int At(int i) { return ids[i]; }
                        }"#;

/// The guard's `ids`, element by element.
fn ids(program: &Program, host: &mut Host) -> Vec<Result<Value, Fault>> {
    let count = match call(program, "Count", &[], host) {
        Ok(Value::Int(n)) => n,
        other => panic!("`Count` returned {other:?}"),
    };
    (0..count)
        .map(|i| call(program, "At", &[Value::Int(i)], host))
        .collect()
}

/// **An argument that waits runs before the field is touched.** `ids.Push(
/// Slow())` and `ids.RemoveAt(Which())` stop at the `await` inside their
/// argument; meanwhile another body writes `ids[0]`. When they resume, the
/// field they change is the field as it is then — that write is kept, never
/// put back by a copy loaded before the wait. On a local, the result is what
/// the body computes.
#[test]
fn an_argument_that_waits_keeps_a_write_made_meanwhile() {
    let program = build(WAITER);
    let cases: [(&str, &[i64]); 2] = [("Grow", &[5, 2, 3, 7]), ("Shrink", &[5, 3])];
    let mut wrong = Vec::new();
    for (member, wanted) in cases {
        let mut host = instance(&program);
        let mut machine = machine_for(&program, member, &[], &host);
        assert_eq!(
            machine.run(&program, &mut host, u64::MAX),
            Run::Suspended(Suspension::Awaiting),
            "`{member}` stops at the await in its argument"
        );
        next_frame(&mut host);
        call(&program, "Poke", &[], &mut host).expect("the other write runs");
        finish(machine, &program, &mut host).expect("the body finishes");
        next_frame(&mut host);

        let want: Vec<Result<Value, Fault>> = wanted.iter().map(|n| Ok(Value::Int(*n))).collect();
        let got = ids(&program, &mut host);
        if got != want {
            wrong.push(format!("`{member}`: ids = {got:?}, expected {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));

    let mut host = instance(&program);
    let local = machine_for(&program, "Local", &[], &host);
    assert_eq!(
        finish(local, &program, &mut host),
        Ok(Value::Int(217)),
        "[1, 2] pushed 7 is [1, 2, 7]; removing index 1 leaves [1, 7]"
    );
}
