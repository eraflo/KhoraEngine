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

//! The Ergon conformance suite: what the language does, construct by construct.
//!
//! Each [`Row`] is a program and what the language does with it — the checker
//! rejects it, the compiler refuses it, or it **runs and returns a value**. This
//! file is the reference: a feature is supported when its row runs, not when
//! the parser accepts it.
//!
//! - [`SHIPPED`] is what the language does today, asserted forever.
//! - [`PENDING`] is what a spec will change. Each row states the behaviour the
//!   spec must deliver and names the spec. When the spec lands, the row starts
//!   producing its target, `pending_rows_are_still_pending` goes red naming it,
//!   and the row moves to `SHIPPED`. A row moves one way only.
//!
//! Only the crate's public API is used, the way the engine uses it:
//! `lex` → `parse` → `check` → `compile` → `Machine::new` → `Machine::run` →
//! `Machine::result`.
//!
//! # Entries
//!
//! A row's `entry` is one step, or several separated by `;`, run in order. The
//! row's value is what the **last** step returned. `args` go to the **first**
//! step; the others are called with none. A step is:
//!
//! | Step | Runs |
//! |---|---|
//! | `F` | the free function `F` |
//! | `B.M` | member `M` of the row's instance of behavior `B`, resolved the way the engine resolves it — the current state's member first, the behavior's own otherwise |
//! | `B.timer#N` | the `N`-th `every` / `after` body `B` declares, in declaration order |
//! | `deliver` | every event the previous steps raised, delivered to the instance |
//!
//! A row whose first step names a behavior gets one instance of it, as the
//! engine creates one: its fields sized from the behavior's layout, its
//! declared defaults run, and an entity to be `this`.
//!
//! A run that suspends is resumed until it completes: for fuel, and for an
//! `await`, which a row treats as having elapsed at once.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::{deliver, resolve_member};
use khora_script::vm::{Machine, Program, Run, Suspension, Value};
use khora_script::{check, compile, lex, parse, Host};

/// What a row expects the language to do.
#[derive(Debug)]
enum Expect {
    /// Compiles, runs to completion, and returns this value.
    Runs(Value),
    /// The compiler refuses it with a message containing this fragment.
    #[allow(dead_code)] // Every refusal the compiler makes today is a pending row.
    Refused(&'static str),
    /// The checker rejects it with a message containing this fragment.
    Rejected(&'static str),
}

/// One construct, and what the language does with it.
struct Row {
    /// What a reader would call it: "break", "array literal".
    construct: &'static str,
    source: &'static str,
    entry: &'static str,
    args: &'static [Value],
    expect: Expect,
}

/// What the language does today.
const SHIPPED: &[Row] = &[
    // ─── Arithmetic ─────────────────────────────────────────────────────────
    Row {
        construct: "integer arithmetic and precedence",
        source: "fn int F() { return 1 + 2 * 3; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(7)),
    },
    Row {
        construct: "parentheses",
        source: "fn int F() { return (1 + 2) * 3; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(9)),
    },
    Row {
        construct: "remainder",
        source: "fn int F() { return 7 % 3; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "unary minus",
        source: "fn int F() { return -5 + 2; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(-3)),
    },
    Row {
        construct: "integer division truncates",
        source: "fn int F() { return 7 / 2; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(3)),
    },
    Row {
        construct: "float division",
        source: "fn float F() { return 7.0 / 2.0; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(3.5)),
    },
    Row {
        construct: "int widened to float",
        source: "fn float F() { return 1 + 0.5; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(1.5)),
    },
    Row {
        construct: "comparison",
        source: "fn bool F() { return 2 <= 2; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "equality",
        source: "fn bool F() { return 1 != 1; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(false)),
    },
    Row {
        construct: "logical not",
        source: "fn bool F() { return !false; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "`&&` short-circuits",
        source: "fn bool F() { return false && true; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(false)),
    },
    Row {
        construct: "`||` short-circuits",
        source: "fn bool F() { return true || false; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    // ─── Variables and control flow ─────────────────────────────────────────
    Row {
        construct: "local and assignment",
        source: "fn int F() { int x = 1; x = x + 4; return x; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(5)),
    },
    Row {
        construct: "`var` and compound assignment",
        source: "fn int F() { var x = 10; x -= 3; return x; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(7)),
    },
    Row {
        construct: "`if` taking the then-branch",
        source: "fn int F() { if (1 < 2) { return 10; } else { return 20; } }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(10)),
    },
    Row {
        construct: "`if` taking the else-branch",
        source: "fn int F() { if (2 < 1) { return 10; } else { return 20; } }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(20)),
    },
    Row {
        construct: "`while`",
        source: "fn int F() {
                     int total = 0;
                     int i = 0;
                     while (i < 5) { total = total + i; i = i + 1; }
                     return total;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(10)),
    },
    Row {
        construct: "`for`",
        source: "fn int F() {
                     int total = 0;
                     for (int i = 0; i < 5; i = i + 1) { total = total + i; }
                     return total;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(10)),
    },
    Row {
        construct: "ternary, condition true",
        source: "fn int F(bool b) { return b ? 1 : 2; }",
        entry: "F",
        args: &[Value::Bool(true)],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "ternary, condition false",
        source: "fn int F(bool b) { return b ? 1 : 2; }",
        entry: "F",
        args: &[Value::Bool(false)],
        expect: Expect::Runs(Value::Int(2)),
    },
    // ─── Functions ──────────────────────────────────────────────────────────
    Row {
        construct: "function call",
        source: "fn int Double(int n) { return n * 2; }
                 fn int F() { return Double(21); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(42)),
    },
    Row {
        construct: "arguments in order",
        source: "fn int Sub(int a, int b) { return a - b; }
                 fn int F() { return Sub(Sub(10, 3), 2); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(5)),
    },
    Row {
        construct: "call to a function declared later",
        source: "fn int F() { return Later(); }
                 fn int Later() { return 9; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(9)),
    },
    Row {
        construct: "recursion",
        source: "fn int Fact(int n) {
                     if (n <= 1) { return 1; }
                     return n * Fact(n - 1);
                 }
                 fn int F() { return Fact(5); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(120)),
    },
    Row {
        construct: "calls inside a loop",
        source: "fn int Double(int n) { return n * 2; }
                 fn int F() {
                     int total = 0;
                     for (int i = 0; i < 5; i = i + 1) { total = total + Double(i); }
                     return total;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(20)),
    },
    Row {
        construct: "argument from the caller",
        source: "fn int F(int n) { return n * 3; }",
        entry: "F",
        args: &[Value::Int(14)],
        expect: Expect::Runs(Value::Int(42)),
    },
    Row {
        construct: "a void function returns nothing",
        source: "fn void F() { int x = 1; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Unit),
    },
    // ─── Values ─────────────────────────────────────────────────────────────
    Row {
        construct: "string concat",
        source: r#"fn bool F() { string a = "hi"; return a + "!" == "hi!"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "`(float)x / 2` divides as floats",
        // What the cast changes today is only which division the compiler
        // picks: `DivFloat`, which widens the int operand, instead of integer
        // division giving `2`. It does not claim the cast converts the value —
        // that is the pending row "cast int to float".
        source: "fn float F(int x) { return (float)x / 2; }",
        entry: "F",
        args: &[Value::Int(5)],
        expect: Expect::Runs(Value::Float(2.5)),
    },
    Row {
        construct: "engine type field",
        source: "fn float F() { Vec3 v = Vec3(1.0, 2.0, 3.0); return v.y; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(2.0)),
    },
    Row {
        construct: "duration arithmetic",
        source: "fn Duration F() { return 2s + 500ms; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(2.5)),
    },
    Row {
        construct: "unit mismatch",
        source: "fn void F() { var wrong = 2s + 90deg; }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("cannot add `Angle` to `Duration`"),
    },
    // ─── Behaviors ──────────────────────────────────────────────────────────
    Row {
        construct: "behavior field default",
        source: "behavior Guard {
                     int health = 100;
                     int Health() { return health; }
                 }",
        entry: "Guard.Health",
        args: &[],
        expect: Expect::Runs(Value::Int(100)),
    },
    Row {
        construct: "behavior method call",
        source: "behavior Car {
                     float Speed() { return 2.5; }
                     float Probe() { return Speed() * 2.0; }
                 }",
        entry: "Car.Probe",
        args: &[],
        expect: Expect::Runs(Value::Float(5.0)),
    },
    Row {
        construct: "`await`",
        source: "behavior Attacker {
                     int landed = 0;
                     async void Attack() {
                         int blow = 41;
                         await 0.5s;
                         landed = blow + 1;
                     }
                     int Landed() { return landed; }
                 }",
        entry: "Attacker.Attack; Attacker.Landed",
        args: &[],
        expect: Expect::Runs(Value::Int(42)),
    },
    Row {
        construct: "`every`",
        source: "behavior Ticker {
                     int count = 0;
                     every 0.5s { count += 1; }
                     int Count() { return count; }
                 }",
        entry: "Ticker.timer#0; Ticker.timer#0; Ticker.Count",
        args: &[],
        expect: Expect::Runs(Value::Int(2)),
    },
    Row {
        construct: "`after`",
        source: "behavior Fuse {
                     bool burnt = false;
                     after 10s { burnt = true; }
                     bool Burnt() { return burnt; }
                 }",
        entry: "Fuse.timer#0; Fuse.Burnt",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "`state`",
        source: "behavior Door {
                     state Closed { int Probe() { return 1; } }
                     state Open { int Probe() { return 2; } }
                 }",
        entry: "Door.Probe",
        args: &[],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "`become`",
        source: "behavior Guard {
                     state Patrol {
                         void Spot() { become Chase(7); }
                         int Prey() { return 0; }
                     }
                     state Chase(int prey) {
                         int Prey() { return prey; }
                     }
                 }",
        entry: "Guard.Spot; Guard.Prey",
        args: &[],
        expect: Expect::Runs(Value::Int(7)),
    },
    Row {
        construct: "`on`",
        source: "behavior Guard {
                     int health = 100;
                     on Damaged(int amount) { health -= amount; }
                     int Health() { return health; }
                 }",
        entry: "Guard.Damaged; Guard.Health",
        args: &[Value::Int(30)],
        expect: Expect::Runs(Value::Int(70)),
    },
    Row {
        construct: "`Raise`",
        source: r#"behavior Guard {
                       int health = 100;
                       on Damaged(int amount) { health -= amount; }
                       void Hurt() { Raise(this, "Damaged", 30); }
                       int Health() { return health; }
                   }"#,
        entry: "Guard.Hurt; deliver; Guard.Health",
        args: &[],
        expect: Expect::Runs(Value::Int(70)),
    },
];

/// What a spec will change. Each row states the behaviour the spec must
/// deliver, and names the spec.
const PENDING: &[(&str, Row)] = &[
    // ─── 02 — runtime correctness ───────────────────────────────────────────
    (
        "02",
        Row {
            construct: "`??` on null",
            source: "fn int F() { int? s = null; return s ?? 7; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(7)),
        },
    ),
    (
        "02",
        Row {
            construct: "uninitialised local",
            source: "fn int F() { int x; x = x + 1; return x; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(1)),
        },
    ),
    (
        "02",
        Row {
            construct: "a non-void function falling off its end",
            source: "fn int F(bool b) { if (b) { return 1; } }",
            entry: "F",
            args: &[Value::Bool(false)],
            expect: Expect::Rejected("does not return a value on every path"),
        },
    ),
    (
        "02",
        Row {
            construct: "`new T(…)`",
            source: r#"fn void F() { var v = new Vec3("x", true); }"#,
            entry: "F",
            args: &[],
            expect: Expect::Rejected("`new` is not supported yet"),
        },
    ),
    (
        "02",
        Row {
            construct: "struct field default of the wrong type",
            source: r#"struct S { int a = "text"; }
                       fn int F() { return 1; }"#,
            entry: "F",
            args: &[],
            expect: Expect::Rejected("expected `int`, found `string`"),
        },
    ),
    (
        // B5: the cast only changes what the compiler believes
        // (`bytecode/expr.rs`, `Expr::Cast`), so an `int` function returns the
        // float it was given.
        "02",
        Row {
            construct: "cast of a float to int",
            source: "fn int F(float x) { return (int)x; }",
            entry: "F",
            args: &[Value::Float(2.75)],
            expect: Expect::Runs(Value::Int(2)),
        },
    ),
    (
        // B5's twin: a `float` function returns the `int` it was given.
        "02",
        Row {
            construct: "cast int to float",
            source: "fn float F(int x) { return (float)x; }",
            entry: "F",
            args: &[Value::Int(5)],
            expect: Expect::Runs(Value::Float(5.0)),
        },
    ),
    // ─── 04 — control flow ──────────────────────────────────────────────────
    (
        "04",
        Row {
            construct: "`break`",
            source: "fn int F() {
                         int total = 0;
                         int i = 0;
                         while (true) {
                             if (i == 5) { break; }
                             total = total + i;
                             i = i + 1;
                         }
                         return total;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(10)),
        },
    ),
    (
        "04",
        Row {
            construct: "`continue` in `while`",
            source: "fn int F() {
                         int total = 0;
                         int i = 0;
                         while (i < 6) {
                             i = i + 1;
                             if (i == 4) { continue; }
                             total = total + i - 1;
                         }
                         return total;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(12)),
        },
    ),
    (
        "04",
        Row {
            construct: "`continue` in `for`",
            source: "fn int F() {
                         int total = 0;
                         for (int i = 0; i < 6; i = i + 1) {
                             if (i == 3) { continue; }
                             total = total + i;
                         }
                         return total;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(12)),
        },
    ),
    (
        "04",
        Row {
            construct: "nested `break`",
            source: "fn int F() {
                         int total = 0;
                         for (int i = 0; i < 3; i = i + 1) {
                             for (int j = 0; j < 10; j = j + 1) {
                                 if (j == 2) { break; }
                                 total = total + 1;
                             }
                         }
                         return total;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(6)),
        },
    ),
    (
        "04",
        Row {
            construct: "`if (var x = opt)` with a value",
            source: "fn int F() {
                         int? opt = 4;
                         if (var x = opt) { return x; }
                         return -1;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(4)),
        },
    ),
    (
        "04",
        Row {
            construct: "`if (var x = opt)` with null",
            source: "fn int F() {
                         int? opt = null;
                         if (var x = opt) { return x; }
                         return -1;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(-1)),
        },
    ),
    (
        "04",
        Row {
            construct: "`while (var x = opt)`",
            source: "behavior Counter {
                         int left = 3;
                         int? Next() {
                             if (left == 0) { return null; }
                             left -= 1;
                             return left;
                         }
                         int Drain() {
                             int count = 0;
                             while (var x = Next()) { count = count + 1; }
                             return count;
                         }
                     }",
            entry: "Counter.Drain",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    (
        "04",
        Row {
            construct: "`match` on an optional with a value",
            source: "behavior Hunter {
                         int Pick(Entity? target) {
                             int picked = 0;
                             match (target) {
                                 Entity e => { picked = 1; }
                                 null => { picked = 2; }
                             }
                             return picked;
                         }
                         int Probe() { return Pick(this); }
                     }",
            entry: "Hunter.Probe",
            args: &[],
            expect: Expect::Runs(Value::Int(1)),
        },
    ),
    (
        "04",
        Row {
            construct: "`match` on an optional with null",
            source: "behavior Hunter {
                         int Pick(Entity? target) {
                             int picked = 0;
                             match (target) {
                                 Entity e => { picked = 1; }
                                 null => { picked = 2; }
                             }
                             return picked;
                         }
                         int Probe() { return Pick(null); }
                     }",
            entry: "Hunter.Probe",
            args: &[],
            expect: Expect::Runs(Value::Int(2)),
        },
    ),
    (
        "04",
        Row {
            construct: "`match` wildcard",
            source: "behavior Hunter {
                         int Pick(Entity? target) {
                             int picked = 0;
                             match (target) {
                                 Entity e => { picked = 1; }
                                 _ => { picked = 3; }
                             }
                             return picked;
                         }
                         int Probe() { return Pick(null); }
                     }",
            entry: "Hunter.Probe",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    // ─── 05 — values ────────────────────────────────────────────────────────
    (
        "05",
        Row {
            construct: "array literal and index",
            source: "fn int F() { return [1, 2, 3][1]; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(2)),
        },
    ),
    (
        "05",
        Row {
            construct: "array `.Length`",
            source: "fn int F() { int[] xs = [1, 2, 3]; return xs.Length; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    (
        "05",
        Row {
            construct: "string `.Length`",
            source: r#"fn int F() { string s = "abc"; return s.Length; }"#,
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    (
        "05",
        Row {
            construct: "`foreach`",
            source: "fn int F() {
                         int sum = 0;
                         foreach (var x in [1, 2, 3]) { sum = sum + x; }
                         return sum;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(6)),
        },
    ),
    (
        "05",
        Row {
            construct: "struct literal and field read",
            source: "struct Loot { int value; int weight = 5; }
                     fn int F() { Loot l = Loot { value: 3 }; return l.value; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    (
        "05",
        Row {
            construct: "struct literal with a missing field",
            source: "struct Loot { int value; int weight = 5; }
                     fn int F() { Loot l = Loot { value: 3 }; return l.weight; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(5)),
        },
    ),
    (
        "05",
        Row {
            construct: "struct field write",
            source: "struct Loot { int value; }
                     fn int F() { Loot l = Loot { value: 3 }; l.value = 9; return l.value; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(9)),
        },
    ),
    (
        "05",
        Row {
            construct: "struct value semantics",
            source: "struct Loot { int value; }
                     fn int F() {
                         Loot a = Loot { value: 3 };
                         Loot b = a;
                         b.value = 9;
                         return a.value;
                     }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    (
        "05",
        Row {
            construct: "`?.` on a present value",
            source: "struct Loot { int value; }
                     fn int? F() { Loot? l = Loot { value: 3 }; return l?.value; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Int(3)),
        },
    ),
    (
        "05",
        Row {
            construct: "`?.` on null",
            source: "struct Loot { int value; }
                     fn int? F() { Loot? l = null; return l?.value; }",
            entry: "F",
            args: &[],
            expect: Expect::Runs(Value::Null),
        },
    ),
];

// ─── The runner ─────────────────────────────────────────────────────────────

/// The entity a behavior row's instance belongs to.
const SUBJECT: EntityId = EntityId {
    index: 1,
    generation: 1,
};

/// The fuel a whole row may spend before it counts as not finishing.
///
/// Far above anything a row needs; low enough that a construct compiled into
/// an endless loop fails the row instead of hanging the suite.
const FUEL_CEILING: u64 = 10_000_000;

/// What the language actually did with a row.
///
/// The messages are read through `Debug`, in the failure reports — which dead
/// code analysis does not count as a read.
#[derive(Debug)]
#[allow(dead_code)]
enum Outcome {
    /// The lexer or the parser refused it.
    Unparsed(Vec<String>),
    /// The checker rejected it.
    Rejected(Vec<String>),
    /// The compiler refused it.
    Refused(Vec<String>),
    /// It ran to completion and returned this.
    Ran(Value),
    /// It compiled but did not complete: a fault, a missing entry, no end.
    Stopped(String),
}

/// Whether `outcome` is what `expect` says.
fn meets(expect: &Expect, outcome: &Outcome) -> bool {
    match (expect, outcome) {
        (Expect::Runs(wanted), Outcome::Ran(got)) => wanted == got,
        (Expect::Refused(fragment), Outcome::Refused(messages))
        | (Expect::Rejected(fragment), Outcome::Rejected(messages)) => {
            messages.iter().any(|message| message.contains(fragment))
        }
        _ => false,
    }
}

/// Runs `row` through the whole pipeline, handing each run at most `slice`
/// fuel before resuming it.
fn outcome(row: &Row, slice: u64) -> Outcome {
    let lexed = lex(row.source);
    if lexed.has_errors() {
        return Outcome::Unparsed(messages(&lexed.diagnostics));
    }
    let parsed = parse(lexed.tokens);
    if parsed.has_errors() {
        return Outcome::Unparsed(messages(&parsed.diagnostics));
    }
    let checked = check(&parsed.module);
    if checked.has_errors() {
        return Outcome::Rejected(messages(&checked.diagnostics));
    }
    let compiled = compile(&parsed.module);
    if compiled.has_errors() {
        return Outcome::Refused(messages(&compiled.diagnostics));
    }

    match execute(&compiled.program, row, slice) {
        Ok(value) => Outcome::Ran(value),
        Err(why) => Outcome::Stopped(why),
    }
}

fn messages(diagnostics: &[khora_script::Diagnostic]) -> Vec<String> {
    diagnostics.iter().map(|d| d.message.clone()).collect()
}

/// Runs the row's steps in order on one host, returning the last one's value.
fn execute(program: &Program, row: &Row, slice: u64) -> Result<Value, String> {
    let steps: Vec<&str> = row.entry.split(';').map(str::trim).collect();
    let mut host = Host::new();
    let mut spent = 0u64;

    // The behavior the row instantiates, if its first step names one.
    let behavior = steps
        .first()
        .and_then(|step| step.split_once('.'))
        .map(|(behavior, _)| behavior)
        .filter(|behavior| program.layout(behavior).is_some());

    if let Some(behavior) = behavior {
        let layout = program.layout(behavior).expect("filtered on its presence");
        host = Host::new()
            .with_fields(PersistentStore::with_slots(layout.slot_count()))
            .for_entity(SUBJECT);
        let machine = Machine::new(program, &init_name(behavior), &[])
            .ok_or_else(|| format!("`{behavior}` has no field initialiser"))?;
        finish(machine, program, &mut host, slice, &mut spent)?;
    }

    let mut last = Value::Unit;
    for (position, step) in steps.iter().enumerate() {
        let args = if position == 0 { row.args } else { &[] };
        last = run_step(program, behavior, step, args, &mut host, slice, &mut spent)?;
    }
    Ok(last)
}

fn run_step(
    program: &Program,
    behavior: Option<&str>,
    step: &str,
    args: &[Value],
    host: &mut Host,
    slice: u64,
    spent: &mut u64,
) -> Result<Value, String> {
    if step == "deliver" {
        let behavior = behavior.ok_or("`deliver` needs a behavior instance")?;
        let events = host.take_events();
        for event in events.as_slice() {
            let delivered = deliver(program, behavior, event, host, slice, |id| id == SUBJECT)
                .map_err(|why| format!("`{}` was not delivered: {why}", event.name))?;
            *spent += delivered.spent;
            match (delivered.outcome, delivered.suspended) {
                (Run::Faulted(fault), _) => {
                    return Err(format!("`{}` faulted: {fault:?}", event.name))
                }
                (_, Some(machine)) => {
                    host.awaiting = None;
                    finish(machine, program, host, slice, spent)?;
                }
                _ => {}
            }
        }
        return Ok(Value::Unit);
    }

    let function = match step.split_once('.') {
        None => step.to_owned(),
        Some((named, member)) => {
            if Some(named) != behavior {
                return Err(format!(
                    "`{step}` is not a member of the row's instance ({behavior:?})"
                ));
            }
            match member.strip_prefix("timer#") {
                Some(index) => {
                    let index: usize = index
                        .parse()
                        .map_err(|_| format!("`{step}`: not a timer index"))?;
                    program
                        .layout(named)
                        .and_then(|layout| layout.timers.get(index))
                        .map(|timer| timer.member.clone())
                        .ok_or_else(|| format!("`{named}` has no timer {index}"))?
                }
                None => resolve_member(program, named, member, host)
                    .ok_or_else(|| format!("`{named}` has no member `{member}`"))?,
            }
        }
    };

    let machine = Machine::new(program, &function, args)
        .ok_or_else(|| format!("no function `{function}` taking {} argument(s)", args.len()))?;
    finish(machine, program, host, slice, spent)
}

/// Resumes `machine` until it completes, `slice` fuel at a time. An `await`
/// is treated as having elapsed.
fn finish(
    mut machine: Machine,
    program: &Program,
    host: &mut Host,
    slice: u64,
    spent: &mut u64,
) -> Result<Value, String> {
    loop {
        let (run, cost) = machine.run_counting(program, host, slice);
        *spent += cost;
        match run {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
            // A fresh slice meets the same instruction again, so a run that
            // could not pay for one never will.
            Run::Suspended(Suspension::OutOfFuel) if cost == 0 => {
                return Err(format!(
                    "no progress with {slice} fuel per run: the next instruction costs more"
                ))
            }
            Run::Suspended(Suspension::OutOfFuel) => {}
            Run::Faulted(fault) => return Err(format!("faulted: {fault:?}")),
        }
        if *spent > FUEL_CEILING {
            return Err(format!("still running after {FUEL_CEILING} fuel"));
        }
    }
}

// ─── The tests ──────────────────────────────────────────────────────────────

/// **The reference.** Every shipped row does exactly what it says.
#[test]
fn every_shipped_row_behaves_as_stated() {
    let failures: Vec<String> = SHIPPED
        .iter()
        .filter_map(|row| {
            let got = outcome(row, FUEL_CEILING);
            (!meets(&row.expect, &got)).then(|| {
                format!(
                    "`{}`: expected {:?}, got {got:?}",
                    row.construct, row.expect
                )
            })
        })
        .collect();

    assert!(
        failures.is_empty(),
        "{} shipped row(s) no longer behave as stated:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// **The invariant the VM rests on, for every construct.** A row that runs
/// returns the same value however finely its execution is sliced: with 1, 2,
/// 3 … 60 fuel per run, resumed until complete.
#[test]
fn every_shipped_row_survives_interruption_anywhere() {
    let mut failures = Vec::new();
    for row in SHIPPED {
        let Expect::Runs(wanted) = &row.expect else {
            continue;
        };
        for slice in 1..=60u64 {
            let got = outcome(row, slice);
            if !matches!(&got, Outcome::Ran(value) if value == wanted) {
                failures.push(format!(
                    "`{}` at a slice of {slice}: expected {wanted:?}, got {got:?}",
                    row.construct
                ));
                // One slice is enough to name the row.
                break;
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} shipped row(s) diverge when interrupted:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

/// **The ledger.** A pending row that already does what its spec promises
/// has landed: move it to `SHIPPED`.
#[test]
fn pending_rows_are_still_pending() {
    let landed: Vec<String> = PENDING
        .iter()
        .filter(|(_, row)| meets(&row.expect, &outcome(row, FUEL_CEILING)))
        .map(|(spec, row)| {
            format!(
                "`{}` (spec {spec}) now behaves as {:?} — move it to SHIPPED",
                row.construct, row.expect
            )
        })
        .collect();

    assert!(
        landed.is_empty(),
        "{} pending row(s) have landed:\n  {}",
        landed.len(),
        landed.join("\n  ")
    );
}
