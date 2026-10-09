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

//! Structs at run time: literals, field reads and writes, `?.`, and what
//! binding one to a name means.
//!
//! A struct is built by a named literal — `Loot { value: 1 }`, fields in any
//! order, a missing field taking its declared default or its type's zero. It
//! is a value (spec 05, D1), like an array: binding one read from a place to a
//! name copies it, so writing through the new name never changes the old one.
//! Writing through a path (`r.points[1].x = 4.0`) changes the struct the path
//! names, in place.

use khora_script::arena::Object;
use khora_script::vm::{Fault, Machine, Program, Run, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// Text no literal below spells, so a string holding it was built running.
#[ergon_fn(name = "StructTextTail")]
fn struct_text_tail() -> String {
    "bcd".to_owned()
}

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

/// Runs the free function `function` of `program` with `args` to completion.
fn run(program: &Program, function: &str, args: &[Value]) -> Result<Value, Fault> {
    let mut machine = Machine::new(program, function, args)
        .unwrap_or_else(|| panic!("`{function}` is a function of the program"));
    let mut host = Host::new();
    for _ in 0..10_000 {
        match machine.run(program, &mut host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("`{function}` never finished");
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

/// Runs every `(function, args, expected)` of `program`, naming each that
/// differs.
fn expect_all(program: &Program, cases: &[(&str, &[Value], Value)]) {
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(function, args, wanted)| {
            let got = run(program, function, args);
            (got.as_ref() != Ok(wanted))
                .then(|| format!("`{function}({args:?})`: expected {wanted:?}, got {got:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The structs every test below shares.
const TYPES: &str = "struct Inner { int value; }
                     struct Outer { Inner inner; int count; }
                     struct Loot { int value; float weight; bool rare; string label; }
                     struct Waypoint { Vec3 at; float wait = 0.5; }
                     struct Bag { int[] xs; }
                     struct Point { float x; float y; }
                     struct Route { Point[] points; }
                     struct Holder { Inner? inner; }
                     struct Score { int? best; }";

/// `TYPES`, then `functions`.
fn with_types(functions: &str) -> String {
    format!("{TYPES}\n{functions}")
}

// ─── Literals and reads ─────────────────────────────────────────────────────

/// **A literal's fields read back as written**, whatever their type and
/// whatever order the literal names them in — a nested struct, an array, an
/// engine value, text built while running — and a literal works wherever an
/// expression does: an argument, a returned value, an array element, a
/// ternary branch, a parenthesised condition, the receiver of a field read.
#[test]
fn a_struct_literal_reads_back_its_fields() {
    let program = build(&with_types(
        r#"fn int Int() { Loot l = Loot { value: 3, weight: 0.5, rare: true, label: "x" }; return l.value; }
           fn float Float() { Loot l = Loot { value: 3, weight: 0.5, rare: true, label: "x" }; return l.weight; }
           fn bool Bool() { Loot l = Loot { value: 3, weight: 0.5, rare: true, label: "x" }; return l.rare; }
           fn int AnyOrder() { Loot l = Loot { label: "x", rare: false, weight: 1.5, value: 7 }; return l.value; }
           fn float Vector() { Waypoint w = Waypoint { at: Vec3(1.0, 2.0, 3.0), wait: 1.0 }; return w.at.y; }
           fn int Nested() { Outer o = Outer { inner: Inner { value: 4 }, count: 2 }; return o.inner.value * 10 + o.count; }
           fn int Holds() { Bag b = Bag { xs: [1, 2, 3] }; return b.xs[1] * 10 + b.xs.Length; }
           fn Inner Make(int v) { return Inner { value: v }; }
           fn int Made() { return Make(6).value; }
           fn int Read(Inner i) { return i.value; }
           fn int Passed() { return Read(Inner { value: 5 }); }
           fn int Direct() { return Inner { value: 8 }.value; }
           fn int Elements() { Inner[] xs = [Inner { value: 1 }, Inner { value: 2 }]; return xs[0].value * 10 + xs[1].value; }
           fn int Chosen(bool pick) { Inner i = pick ? Inner { value: 1 } : Inner { value: 2 }; return i.value; }
           fn int Condition() { if (Read(Inner { value: 3 }) == 3) { return 1; } return 0; }
           fn int Inferred() { var i = Inner { value: 9 }; return i.value; }"#,
    ));
    expect_all(
        &program,
        &[
            ("Int", &[], Value::Int(3)),
            ("Float", &[], Value::Float(0.5)),
            ("Bool", &[], Value::Bool(true)),
            ("AnyOrder", &[], Value::Int(7)),
            ("Vector", &[], Value::Float(2.0)),
            ("Nested", &[], Value::Int(42)),
            ("Holds", &[], Value::Int(23)),
            ("Made", &[], Value::Int(6)),
            ("Passed", &[], Value::Int(5)),
            ("Direct", &[], Value::Int(8)),
            ("Elements", &[], Value::Int(12)),
            ("Chosen", &[Value::Bool(true)], Value::Int(1)),
            ("Chosen", &[Value::Bool(false)], Value::Int(2)),
            ("Condition", &[], Value::Int(1)),
            ("Inferred", &[], Value::Int(9)),
        ],
    );

    let text = build(&with_types(
        r#"fn string Built() {
               Loot l = Loot { value: 1, weight: 0.0, rare: false, label: "a" + StructTextTail() };
               return l.label;
           }"#,
    ));
    assert_eq!(run_text(&text, "Built"), Ok("abcd".to_owned()));
}

// ─── Defaults and zeros ─────────────────────────────────────────────────────

/// **A field the literal leaves out takes its declared default** — a literal,
/// a negated literal, `null`, a duration, an array or a struct of constants —
/// or, without one, its type's zero. A field the literal names takes what it
/// was given. Each literal gets its own copy of a default: writing into one
/// struct's default array leaves the next literal's alone.
#[test]
fn a_struct_literal_takes_its_constant_defaults() {
    let program = build(
        r#"struct Inner { int value; }
           struct Loot {
               int value;
               int weight = 5;
               float scale = -1.5;
               bool rare = true;
               string label = "plain";
               int? best = null;
               Duration pause = 2s;
               int[] xs = [1, 2];
               Inner inner = Inner { value: 7 };
           }
           struct Zeros { int n; float f; bool b; string s; int? o; int[] xs; }
           fn int Weight() { Loot l = Loot { value: 1 }; return l.weight; }
           fn float Scale() { Loot l = Loot { value: 1 }; return l.scale; }
           fn bool Rare() { Loot l = Loot { value: 1 }; return l.rare; }
           fn int Label() { Loot l = Loot { value: 1 }; return l.label.Length; }
           fn int Best() { Loot l = Loot { value: 1 }; return l.best ?? -1; }
           fn float Pause() { Loot l = Loot { value: 1 }; return l.pause.Seconds; }
           fn int Xs() { Loot l = Loot { value: 1 }; return l.xs[1] * 10 + l.xs.Length; }
           fn int InnerValue() { Loot l = Loot { value: 1 }; return l.inner.value; }
           fn int Given() { Loot l = Loot { value: 1, weight: 9 }; return l.weight; }
           fn int ZeroInt() { Zeros z = Zeros { }; return z.n; }
           fn float ZeroFloat() { Zeros z = Zeros { }; return z.f; }
           fn bool ZeroBool() { Zeros z = Zeros { b: false }; return z.b; }
           fn int ZeroText() { Zeros z = Zeros { }; return z.s.Length; }
           fn int ZeroOptional() { Zeros z = Zeros { }; return z.o ?? -1; }
           fn int ZeroArray() { Zeros z = Zeros { }; return z.xs.Length; }
           fn int Separate() {
               Loot a = Loot { value: 1 };
               a.xs[0] = 9;
               a.inner.value = 8;
               Loot b = Loot { value: 2 };
               return b.xs[0] * 10 + b.inner.value;
           }"#,
    );
    expect_all(
        &program,
        &[
            ("Weight", &[], Value::Int(5)),
            ("Scale", &[], Value::Float(-1.5)),
            ("Rare", &[], Value::Bool(true)),
            ("Label", &[], Value::Int(5)),
            ("Best", &[], Value::Int(-1)),
            ("Pause", &[], Value::Float(2.0)),
            ("Xs", &[], Value::Int(22)),
            ("InnerValue", &[], Value::Int(7)),
            ("Given", &[], Value::Int(9)),
            ("ZeroInt", &[], Value::Int(0)),
            ("ZeroFloat", &[], Value::Float(0.0)),
            ("ZeroBool", &[], Value::Bool(false)),
            ("ZeroText", &[], Value::Int(0)),
            ("ZeroOptional", &[], Value::Int(-1)),
            ("ZeroArray", &[], Value::Int(0)),
            ("Separate", &[], Value::Int(17)),
        ],
    );
}

// ─── Writes ─────────────────────────────────────────────────────────────────

/// **A write through a path lands in the struct the path names** — a field of
/// a local, a compound write, a nested field, an element's field, a field's
/// element, `r.points[1].x`, a whole nested struct replaced, an engine value
/// replaced whole — and only there.
#[test]
fn field_writes_land_in_place() {
    let program = build(&with_types(
        "fn int Local() { Inner i = Inner { value: 3 }; i.value = 9; return i.value; }
         fn int Compound() { Inner i = Inner { value: 3 }; i.value += 4; return i.value; }
         fn int Nested() {
             Outer o = Outer { inner: Inner { value: 1 }, count: 2 };
             o.inner.value = 5;
             return o.inner.value * 10 + o.count;
         }
         fn float Path() {
             Route r = Route { points: [Point { x: 1.0 }, Point { x: 2.0 }] };
             r.points[1].x = 4.0;
             return r.points[1].x + r.points[0].x * 10.0 + r.points[1].y * 100.0;
         }
         fn int ElementField() {
             Inner[] xs = [Inner { value: 1 }, Inner { value: 2 }];
             xs[1].value = 7;
             return xs[0].value * 10 + xs[1].value;
         }
         fn int Indexed(int i) {
             Inner[] xs = [Inner { value: 1 }, Inner { value: 2 }];
             xs[i].value += 10;
             return xs[0].value * 100 + xs[1].value;
         }
         fn int FieldElement() { Bag b = Bag { xs: [1, 2, 3] }; b.xs[1] = 8; return b.xs[0] * 10 + b.xs[1]; }
         fn int Whole() {
             Outer o = Outer { inner: Inner { value: 1 }, count: 3 };
             o.inner = Inner { value: 6 };
             return o.inner.value * 10 + o.count;
         }
         fn float Component() {
             Waypoint w = Waypoint { at: Vec3(1.0, 0.0, 0.0) };
             w.at = Vec3(4.0, 0.0, 0.0);
             return w.at.x + w.wait;
         }",
    ));
    expect_all(
        &program,
        &[
            ("Local", &[], Value::Int(9)),
            ("Compound", &[], Value::Int(7)),
            ("Nested", &[], Value::Int(52)),
            ("Path", &[], Value::Float(14.0)),
            ("ElementField", &[], Value::Int(17)),
            ("Indexed", &[Value::Int(1)], Value::Int(112)),
            ("Indexed", &[Value::Int(0)], Value::Int(1102)),
            ("FieldElement", &[], Value::Int(18)),
            ("Whole", &[], Value::Int(63)),
            ("Component", &[], Value::Float(4.5)),
        ],
    );
}

// ─── Value semantics ────────────────────────────────────────────────────────

/// **D1, for structs.** Binding a struct read from a place copies it: a `let`,
/// an assignment, an argument, an element bound whole, a field bound whole, a
/// `foreach` element, a ternary's and a `??`'s result, a narrowing and a
/// `match` binding — and a place put into a literal. Writing through the new
/// name leaves the original as it was, arrays inside it included.
#[test]
fn structs_have_value_semantics() {
    let program = build(&with_types(
        "fn int Let() { Inner a = Inner { value: 1 }; Inner b = a; b.value = 9; return a.value * 10 + b.value; }
         fn int Assign() {
             Inner a = Inner { value: 1 };
             Inner b = Inner { value: 0 };
             b = a;
             b.value = 9;
             return a.value * 10 + b.value;
         }
         fn int Poke(Inner i) { i.value = 9; return i.value; }
         fn int Argument() { Inner a = Inner { value: 1 }; int seen = Poke(a); return a.value * 10 + seen; }
         fn int Element() { Inner[] xs = [Inner { value: 1 }]; Inner x = xs[0]; x.value = 9; return xs[0].value; }
         fn int Member() {
             Outer o = Outer { inner: Inner { value: 1 }, count: 0 };
             Inner i = o.inner;
             i.value = 9;
             return o.inner.value;
         }
         fn int Each() {
             Inner[] xs = [Inner { value: 1 }, Inner { value: 2 }];
             foreach (var x in xs) { x.value = 9; }
             return xs[0].value * 10 + xs[1].value;
         }
         fn int Ternary(bool pick) { Inner a = Inner { value: 1 }; Inner b = pick ? a : a; b.value = 9; return a.value; }
         fn int Fallback() {
             Inner? none = null;
             Inner a = Inner { value: 1 };
             Inner b = none ?? a;
             b.value = 9;
             return a.value;
         }
         fn int Narrowed() {
             Inner? maybe = Inner { value: 1 };
             if (var x = maybe) { x.value = 9; }
             return maybe?.value ?? 0;
         }
         fn int Matched() {
             Inner? maybe = Inner { value: 1 };
             match (maybe) {
                 Inner x => { x.value = 9; }
                 null => { }
             }
             return maybe?.value ?? 0;
         }
         fn int InLiteral() {
             Inner a = Inner { value: 1 };
             Outer o = Outer { inner: a, count: 0 };
             o.inner.value = 9;
             return a.value * 10 + o.inner.value;
         }
         fn int InArray() { Inner a = Inner { value: 1 }; Inner[] xs = [a]; xs[0].value = 9; return a.value; }
         fn int Back() { Inner a = Inner { value: 1 }; Inner b = a; a.value = 5; return b.value; }
         fn int Deep() { Bag a = Bag { xs: [1] }; Bag b = a; b.xs[0] = 9; return a.xs[0]; }",
    ));
    expect_all(
        &program,
        &[
            ("Let", &[], Value::Int(19)),
            ("Assign", &[], Value::Int(19)),
            ("Argument", &[], Value::Int(19)),
            ("Element", &[], Value::Int(1)),
            ("Member", &[], Value::Int(1)),
            ("Each", &[], Value::Int(12)),
            ("Ternary", &[Value::Bool(true)], Value::Int(1)),
            ("Fallback", &[], Value::Int(1)),
            ("Narrowed", &[], Value::Int(1)),
            ("Matched", &[], Value::Int(1)),
            ("InLiteral", &[], Value::Int(19)),
            ("InArray", &[], Value::Int(1)),
            ("Back", &[], Value::Int(1)),
            ("Deep", &[], Value::Int(1)),
        ],
    );
}

// ─── `?.` ───────────────────────────────────────────────────────────────────

/// **`?.` reads a field of a struct that may be absent**: the field when it
/// is there, `null` when it is not — chained, on a call's result, and on a
/// field that is itself optional, where "absent struct" and "absent field"
/// are the same `null` (`T??` is `T?`).
#[test]
fn optional_field_access_reads_or_gives_null() {
    let program = build(&with_types(
        "fn int? Present() { Inner? l = Inner { value: 3 }; return l?.value; }
         fn int? Absent() { Inner? l = null; return l?.value; }
         fn int? Chain(int which) {
             Holder? h = null;
             if (which == 1) { h = Holder { }; }
             if (which == 2) { h = Holder { inner: Inner { value: 4 } }; }
             return h?.inner?.value;
         }
         fn int Best(int which) {
             Score? s = null;
             if (which == 1) { s = Score { }; }
             if (which == 2) { s = Score { best: 6 }; }
             int? b = s?.best;
             return b ?? -1;
         }
         fn Inner? Find(bool has) { if (has) { return Inner { value: 2 }; } return null; }
         fn int OnCall(bool has) { return Find(has)?.value ?? -1; }",
    ));
    expect_all(
        &program,
        &[
            ("Present", &[], Value::Int(3)),
            ("Absent", &[], Value::Null),
            ("Chain", &[Value::Int(0)], Value::Null),
            ("Chain", &[Value::Int(1)], Value::Null),
            ("Chain", &[Value::Int(2)], Value::Int(4)),
            ("Best", &[Value::Int(0)], Value::Int(-1)),
            ("Best", &[Value::Int(1)], Value::Int(-1)),
            ("Best", &[Value::Int(2)], Value::Int(6)),
            ("OnCall", &[Value::Bool(true)], Value::Int(2)),
            ("OnCall", &[Value::Bool(false)], Value::Int(-1)),
        ],
    );
}

// ─── Fuel ───────────────────────────────────────────────────────────────────

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

/// **Every place a fuel cut can land, in a struct program.** At every slice
/// from 1 to 60, each body returns what it returns run whole. Between slices
/// the frame's arena is left as it is: a run resumed in the same frame finds
/// its structs where it left them, beside other objects built meanwhile.
#[test]
fn struct_programs_return_the_same_at_every_fuel_slice() {
    let program = build(&with_types(
        "fn float Path() {
             Route r = Route { points: [Point { x: 1.0 }, Point { x: 2.0 }] };
             r.points[1].x = 4.0;
             Route copy = r;
             copy.points[0].x = 9.0;
             return r.points[1].x + r.points[0].x * 10.0 + copy.points[0].x * 100.0;
         }
         fn int Each() {
             Inner[] xs = [Inner { value: 1 }, Inner { value: 2 }, Inner { value: 3 }];
             int sum = 0;
             foreach (var x in xs) { x.value += 10; sum = sum * 100 + x.value; }
             return sum + xs[2].value;
         }
         fn int Nested() {
             Outer o = Outer { inner: Inner { value: 1 }, count: 2 };
             Outer p = o;
             p.inner.value = 7;
             return o.inner.value * 100 + p.inner.value * 10 + p.count;
         }",
    ));
    let cases = [
        ("Path", Value::Float(914.0)),
        ("Each", Value::Int(111_216)),
        ("Nested", Value::Int(172)),
    ];
    let mut wrong = Vec::new();
    for (function, wanted) in cases {
        for slice in 1..=60u64 {
            let mut machine = Machine::new(&program, function, &[]).expect("the function exists");
            let mut host = Host::new();
            let mut got = Err("never finished".to_owned());
            for _ in 0..100_000 {
                match machine.run(&program, &mut host, slice) {
                    Run::Suspended(_) => occupy(&mut host),
                    Run::Completed => {
                        got = Ok(machine.result());
                        break;
                    }
                    Run::Faulted(fault) => {
                        got = Err(format!("{fault:?}"));
                        break;
                    }
                }
            }
            if got.as_ref() != Ok(&wanted) {
                wrong.push(format!("`{function}` at slice {slice}: {got:?}"));
                break;
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
