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
//! engine creates one: its store built from the behavior's layout, every
//! field at its default, and an entity to be `this`.
//!
//! A run that suspends is resumed until it completes: for fuel, and for an
//! `await`, which a row treats as having elapsed at once.

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
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
    /// The lexer or the parser refuses it with a message containing this
    /// fragment.
    Unparsed(&'static str),
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
        // The cast picks the division: `DivFloat` instead of integer division
        // giving `2`. That the cast converts the value is the row "cast int to
        // float".
        source: "fn float F(int x) { return (float)x / 2; }",
        entry: "F",
        args: &[Value::Int(5)],
        expect: Expect::Runs(Value::Float(2.5)),
    },
    Row {
        construct: "`??` on null",
        source: "fn int F() { int? s = null; return s ?? 7; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(7)),
    },
    Row {
        construct: "cast of a float to int",
        source: "fn int F(float x) { return (int)x; }",
        entry: "F",
        args: &[Value::Float(2.75)],
        expect: Expect::Runs(Value::Int(2)),
    },
    Row {
        construct: "cast int to float",
        source: "fn float F(int x) { return (float)x; }",
        entry: "F",
        args: &[Value::Int(5)],
        expect: Expect::Runs(Value::Float(5.0)),
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
        source: "event Damaged(int amount);
                 behavior Guard {
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
        source: r#"event Damaged(int amount);
                   behavior Guard {
                       int health = 100;
                       on Damaged(int amount) { health -= amount; }
                       void Hurt() { this.Raise(Damaged(30)); }
                       int Health() { return health; }
                   }"#,
        entry: "Guard.Hurt; deliver; Guard.Health",
        args: &[],
        expect: Expect::Runs(Value::Int(70)),
    },
    Row {
        construct: "`Raise` widening an `int` to the event's `float`",
        source: r#"event Healed(float amount);
                   behavior Medic {
                       float health = 10.0;
                       on Healed(float amount) { health += amount; }
                       void Heal() { this.Raise(Healed(5)); }
                       float Health() { return health; }
                   }"#,
        entry: "Medic.Heal; deliver; Medic.Health",
        args: &[],
        expect: Expect::Runs(Value::Float(15.0)),
    },
    Row {
        construct: "a handler ignoring its event's payload",
        source: r#"event Damaged(int amount);
                   behavior Guard {
                       int hits = 0;
                       on Damaged { hits += 1; }
                       void Hurt() { this.Raise(Damaged(30)); }
                       int Hits() { return hits; }
                   }"#,
        entry: "Guard.Hurt; deliver; Guard.Hits",
        args: &[],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "`Raise` by name",
        source: r#"behavior Guard {
                       void Hurt() { this.Raise("Damaged", 30); }
                   }"#,
        entry: "Guard.Hurt",
        args: &[],
        expect: Expect::Rejected("event Damaged"),
    },
    Row {
        construct: "a handler of an undeclared event",
        source: "behavior Guard { on Damaged(int amount) { } }",
        entry: "Guard.Damaged",
        args: &[Value::Int(1)],
        expect: Expect::Rejected("no event named `Damaged`"),
    },
    Row {
        construct: "`await` of something other than a duration",
        source: "behavior Waiter { async void Wait() { await true; } }",
        entry: "Waiter.Wait",
        args: &[],
        expect: Expect::Rejected("`await` takes a duration"),
    },
    // ─── Runtime correctness (spec 02) ──────────────────────────────────────
    Row {
        construct: "uninitialised local",
        source: "fn int F() { int x; x = x + 1; return x; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "a non-void function falling off its end",
        source: "fn int F(bool b) { if (b) { return 1; } }",
        entry: "F",
        args: &[Value::Bool(false)],
        expect: Expect::Rejected("does not return a value on every path"),
    },
    Row {
        construct: "`new T(…)`",
        source: r#"fn void F() { var v = new Vec3("x", true); }"#,
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`new` is not supported yet"),
    },
    Row {
        construct: "struct field default of the wrong type",
        source: r#"struct S { int a = "text"; }
                   fn int F() { return 1; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Rejected("expected `int`, found `string`"),
    },
    // ─── Loops, narrowing and `match` (spec 04) ─────────────────────────────
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
    // ─── Values (spec 05) ───────────────────────────────────────────────────
    Row {
        construct: "array literal and index",
        source: "fn int F() { return [1, 2, 3][1]; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(2)),
    },
    Row {
        construct: "array `.Length`",
        source: "fn int F() { int[] xs = [1, 2, 3]; return xs.Length; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(3)),
    },
    Row {
        construct: "string `.Length`",
        source: r#"fn int F() { string s = "abc"; return s.Length; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(3)),
    },
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
    Row {
        construct: "struct literal and field read",
        source: "struct Loot { int value; int weight = 5; }
                 fn int F() { Loot l = Loot { value: 3 }; return l.value; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(3)),
    },
    Row {
        construct: "struct literal with a missing field",
        source: "struct Loot { int value; int weight = 5; }
                 fn int F() { Loot l = Loot { value: 3 }; return l.weight; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(5)),
    },
    Row {
        construct: "struct field write",
        source: "struct Loot { int value; }
                 fn int F() { Loot l = Loot { value: 3 }; l.value = 9; return l.value; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(9)),
    },
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
    Row {
        construct: "`?.` on a present value",
        source: "struct Loot { int value; }
                 fn int? F() { Loot? l = Loot { value: 3 }; return l?.value; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(3)),
    },
    Row {
        construct: "`?.` on null",
        source: "struct Loot { int value; }
                 fn int? F() { Loot? l = null; return l?.value; }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Null),
    },
    Row {
        construct: "nested path write",
        source: "struct Point { float x; float y; }
                 struct Route { Point[] points; }
                 fn float F() {
                     Route r = Route { points: [Point { x: 1.0, y: 0.0 }, Point { x: 2.0, y: 0.0 }] };
                     r.points[1].x = 4.0;
                     return r.points[1].x;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(4.0)),
    },
    // ─── Entity methods and component writes (spec 06) ──────────────────────
    Row {
        construct: "an entity operation as a method",
        source: "behavior Mover {
                     int Go() { this.Translate(Vec3(0.0, 1.0, 0.0)); return 1; }
                 }",
        entry: "Mover.Go",
        args: &[],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "a method on an entity held in a variable",
        source: "behavior Mover {
                     int Go() { Entity e = this; e.SetPosition(Vec3(1.0, 2.0, 3.0)); e.Despawn(); return 2; }
                 }",
        entry: "Mover.Go",
        args: &[],
        expect: Expect::Runs(Value::Int(2)),
    },
    Row {
        construct: "`SetRotation`",
        source: "behavior Turret {
                     int Aim() { this.SetRotation(Quat(0.0, 0.6, 0.0, 0.8)); return 3; }
                 }",
        entry: "Turret.Aim",
        args: &[],
        expect: Expect::Runs(Value::Int(3)),
    },
    Row {
        construct: "the free form of an entity operation",
        source: "behavior Mover {
                     void Go() { Vec3 v = Vec3(1.0, 0.0, 0.0); SetPosition(this, v); }
                 }",
        entry: "Mover.Go",
        args: &[],
        expect: Expect::Rejected("Write `this.SetPosition(v)`"),
    },
    Row {
        construct: "the free form of `Raise`",
        source: r#"event Damaged(int amount);
                   behavior Guard {
                       on Damaged(int amount) { }
                       void Hurt() { Raise(this, Damaged(30)); }
                   }"#,
        entry: "Guard.Hurt",
        args: &[],
        expect: Expect::Rejected("Write `this.Raise(Damaged(30))`"),
    },
    Row {
        construct: "`Set`",
        source: "component Health { int current; int max; }
                 behavior Guard {
                     int Heal() { this.Set(Health { current: 5 }); return 5; }
                 }",
        entry: "Guard.Heal",
        args: &[],
        expect: Expect::Runs(Value::Int(5)),
    },
    Row {
        construct: "`Add`",
        source: "component Health { int current; int max; }
                 behavior Guard {
                     int Arm() { this.Add(Health { max: 10, current: 10 }); return 10; }
                 }",
        entry: "Guard.Arm",
        args: &[],
        expect: Expect::Runs(Value::Int(10)),
    },
    Row {
        construct: "`Remove`",
        source: "component Health { int current; int max; }
                 behavior Guard {
                     int Disarm() { this.Remove(Health); return 0; }
                 }",
        entry: "Guard.Disarm",
        args: &[],
        expect: Expect::Runs(Value::Int(0)),
    },
    Row {
        construct: "`Spawn`",
        source: "component Health { int current; int max; }
                 fn int F() {
                     Spawn(Vec3(0.0, 1.0, 0.0), Health { current: 3 });
                     Spawn(Vec3(0.0, 2.0, 0.0));
                     return 2;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(2)),
    },
    Row {
        construct: "`Spawn` called on an entity",
        source: "behavior Spawner { void Go() { this.Spawn(Vec3(0.0, 0.0, 0.0)); } }",
        entry: "Spawner.Go",
        args: &[],
        expect: Expect::Rejected("`Spawn`"),
    },
    Row {
        construct: "`Spawn` returns its entity, written to in the same turn",
        source: "component Health { int current; int max; }
                 behavior Spawner {
                     int Go() {
                         var b = Spawn(Vec3(0.0, 1.0, 0.0), Health { current: 3 });
                         b.Set(Health { current: 4 });
                         b.SetPosition(Vec3(1.0, 0.0, 0.0));
                         return 2;
                     }
                 }",
        entry: "Spawner.Go",
        args: &[],
        expect: Expect::Runs(Value::Int(2)),
    },
    Row {
        construct: "`Spawn` as an `Entity`",
        source: "fn int F() {
                     Entity e = Spawn(Vec3(0.0, 0.0, 0.0));
                     Entity? f = Spawn(Vec3(0.0, 0.0, 0.0));
                     return 1;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(1)),
    },
    Row {
        construct: "`Spawn` as a value of another type",
        source: "fn void F() { int e = Spawn(Vec3(0.0, 0.0, 0.0)); }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("expected `int`, found `Entity`"),
    },
    Row {
        construct: "a component as a variable's type",
        source: "component Health { int current; int max; }
                 fn void F() { Health h; }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`Health` is a component"),
    },
    Row {
        construct: "`Set` of a plain struct",
        source: "struct Loot { int value; }
                 behavior Guard { void Go() { this.Set(Loot { value: 1 }); } }",
        entry: "Guard.Go",
        args: &[],
        expect: Expect::Rejected("`Loot` is not a component a script writes"),
    },
    Row {
        construct: "a function named like an intrinsic",
        source: "fn void Set() { }",
        entry: "Set",
        args: &[],
        expect: Expect::Rejected("`Set`"),
    },
    // ─── Component reads (spec 07) ──────────────────────────────────────────
    // A row's host projects no component, so every entity holds none.
    Row {
        construct: "`Get` of a component the entity does not hold",
        source: "component Health { int current; int max; }
                 behavior Medic {
                     int Look() { if (var h = this.Get(Health)) { return h.current; } return -1; }
                 }",
        entry: "Medic.Look",
        args: &[],
        expect: Expect::Runs(Value::Int(-1)),
    },
    Row {
        construct: "`Has`",
        source: "component Health { int current; int max; }
                 behavior Medic { bool Probe() { return this.Has(Health); } }",
        entry: "Medic.Probe",
        args: &[],
        expect: Expect::Runs(Value::Bool(false)),
    },
    Row {
        construct: "a component-typed local holding a `Get`",
        source: "component Health { int current; int max; }
                 behavior Medic {
                     int Look() { Health? h = this.Get(Health); return h?.current ?? 7; }
                 }",
        entry: "Medic.Look",
        args: &[],
        expect: Expect::Runs(Value::Int(7)),
    },
    Row {
        construct: "`Get` on an entity held in a variable",
        source: "component Health { int current; int max; }
                 fn int F(Entity e) { var h = e.Get(Health); return h?.max ?? 3; }",
        entry: "F",
        args: &[Value::Entity(SUBJECT)],
        expect: Expect::Runs(Value::Int(3)),
    },
    Row {
        construct: "a field of a copy written",
        source: "component Health { int current; int max; }
                 behavior Medic {
                     void Heal() { if (var h = this.Get(Health)) { h.current = 5; } }
                 }",
        entry: "Medic.Heal",
        args: &[],
        expect: Expect::Rejected("`h` is a copy of `Health` and is read-only"),
    },
    Row {
        construct: "`Get` of a plain struct",
        source: "struct Loot { int value; }
                 behavior Guard { void Go() { var l = this.Get(Loot); } }",
        entry: "Guard.Go",
        args: &[],
        expect: Expect::Rejected("`Loot` is not a component"),
    },
    // ─── Named arguments and parameter defaults ─────────────────────────────
    Row {
        construct: "named arguments in any order",
        source: "fn int Pair(int a, int b) { return a * 10 + b; }
                 fn int F() { return Pair(b: 4, a: 3); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(34)),
    },
    Row {
        construct: "positional arguments, then named ones",
        source: "fn int Triple(int a, int b, int c) { return a * 100 + b * 10 + c; }
                 fn int F() { return Triple(1, c: 3, b: 2); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(123)),
    },
    Row {
        construct: "a parameter default",
        source: "fn int Move(int x, int speed = 3) { return x * 10 + speed; }
                 fn int F() { return Move(4); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(43)),
    },
    Row {
        construct: "a default skipped by naming the parameter after it",
        source: "fn int Move(int x, int speed = 3, int turns = 5) {
                     return x * 100 + speed * 10 + turns;
                 }
                 fn int F() { return Move(1, turns: 7); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Int(137)),
    },
    Row {
        construct: "named arguments evaluated in the order written",
        source: "behavior Recorder {
                     int trace = 0;
                     int Mark(int digit) { trace = trace * 10 + digit; return digit; }
                     int Pair(int first, int second) { return first * 10 + second; }
                     int Probe() {
                         int got = Pair(second: Mark(2), first: Mark(1));
                         return trace * 100 + got;
                     }
                 }",
        entry: "Recorder.Probe",
        args: &[],
        expect: Expect::Runs(Value::Int(2112)),
    },
    Row {
        construct: "a behavior method with a default, called with a name",
        source: "behavior Car {
                     int Score(int laps, int bonus = 2) { return laps * 10 + bonus; }
                     int Probe() { return Score(laps: 4); }
                 }",
        entry: "Car.Probe",
        args: &[],
        expect: Expect::Runs(Value::Int(42)),
    },
    Row {
        construct: "an engine function's parameters named",
        source: "fn float F() { return Lerp(t: 0.25, to: 8.0, from: 0.0); }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(2.0)),
    },
    Row {
        construct: "an engine constructor's fields named",
        source: "fn float F() {
                     Vec3 v = Vec3(z: 3.0, x: 1.0, y: 2.0);
                     return v.x * 100.0 + v.y * 10.0 + v.z;
                 }",
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Float(123.0)),
    },
    Row {
        construct: "an event's payload named",
        source: "event Struck(int damage, int times);
                 behavior Guard {
                     int health = 100;
                     on Struck(int damage, int times) { health -= damage * times; }
                     void Hurt() { this.Raise(Struck(times: 2, damage: 7)); }
                     int Health() { return health; }
                 }",
        entry: "Guard.Hurt; deliver; Guard.Health",
        args: &[],
        expect: Expect::Runs(Value::Int(86)),
    },
    Row {
        construct: "`become` with its state's data named",
        source: "behavior Guard {
                     state Patrol {
                         void Spot() { become Chase(prey: 7, range: 2); }
                         int Read() { return 0; }
                     }
                     state Chase(int range, int prey) {
                         int Read() { return prey * 10 + range; }
                     }
                 }",
        entry: "Guard.Spot; Guard.Read",
        args: &[],
        expect: Expect::Runs(Value::Int(72)),
    },
    Row {
        construct: "a positional argument after a named one",
        source: "fn int Pair(int a, int b) { return a + b; }
                 fn int F() { return Pair(a: 1, 2); }",
        entry: "F",
        args: &[],
        expect: Expect::Unparsed("a positional argument cannot follow a named one"),
    },
    Row {
        construct: "a parameter given twice",
        source: "fn int Pair(int a, int b) { return a + b; }
                 fn int F() { return Pair(1, a: 2, b: 3); }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`a` is given twice"),
    },
    Row {
        construct: "a name the callee does not have",
        source: "fn int Move(int x, float speed) { return x; }
                 fn int F() { return Move(1, sped: 2.0); }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`Move` has no parameter `sped`"),
    },
    Row {
        construct: "a parameter without a default left out",
        source: "fn int Move(int x, float speed) { return x; }
                 fn int F() { return Move(x: 1); }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`Move` needs `speed`"),
    },
    Row {
        construct: "a default that is not a constant",
        source: "fn int Base() { return 1; }
                 fn int F(int x = Base()) { return x; }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("default must be a constant"),
    },
    Row {
        construct: "a parameter without a default after one with a default",
        source: "fn int F(int x = 1, int y) { return x + y; }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`y` needs a default: it follows `x`, which has one"),
    },
    Row {
        construct: "a lifecycle method's parameter with a default",
        source: "behavior Guard { void Update(float dt = 0.016) { } }",
        entry: "Guard.Update",
        args: &[],
        expect: Expect::Rejected("a lifecycle method is called by the engine"),
    },
    Row {
        construct: "a handler's parameter with a default",
        source: "event Damaged(int amount);
                 behavior Guard { on Damaged(int amount = 1) { } }",
        entry: "Guard.Damaged",
        args: &[],
        expect: Expect::Rejected("a handler's parameters are its event's payload"),
    },
    Row {
        construct: "an event's parameter with a default",
        source: "event Damaged(int amount = 1);
                 fn int F() { return 1; }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("an event's payload is given whole when raised"),
    },
    Row {
        construct: "an array method given a name",
        source: "fn int F() { int[] xs = [1]; xs.Push(value: 2); return xs.Length; }",
        entry: "F",
        args: &[],
        expect: Expect::Rejected("`Push` takes no named argument"),
    },
    Row {
        construct: "`become` with a value of the wrong type",
        source: r#"behavior Guard {
                       state Patrol { void Spot() { become Chase("seven"); } }
                       state Chase(int prey) { int Prey() { return prey; } }
                   }"#,
        entry: "Guard.Spot",
        args: &[],
        expect: Expect::Rejected("expected `int`, found `string`"),
    },
    // ─── String interpolation ───────────────────────────────────────────────
    Row {
        construct: "string interpolation",
        source: r#"fn bool F() { int hp = 3; return $"hp: {hp}!" == "hp: 3!"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "an interpolated float keeps its decimal point",
        source: r#"fn bool F() { float x = 2.0; return $"{x} {0.1}" == "2.0 0.1"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "an interpolated duration and angle show their unit",
        source: r#"fn bool F() {
                       Duration d = 2.5s;
                       Angle a = 90deg;
                       return $"{d} {a}" == "2.5s 90deg";
                   }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "an interpolated empty optional is `null`",
        source: r#"fn bool F() { int? n = null; return $"{n}" == "null"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "interpolated engine values",
        source: r#"fn bool F() {
                       Vec3 v = Vec3(1.0, 2.0, 3.0);
                       Color c = Color(1.0, 0.5, 0.0, 1.0);
                       return $"{v} {c}" == "(1.0, 2.0, 3.0) rgba(1.0, 0.5, 0.0, 1.0)";
                   }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "an interpolated bool and string",
        source: r#"fn bool F() { string s = "on"; return $"{s}={true}" == "on=true"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "doubled braces in an interpolation",
        source: r#"fn bool F() { int n = 3; return $"{{{n}}}" == "{3}"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "an interpolation inside a hole",
        source: r#"fn bool F() { return $"<{$"{1}"}>" == "<1>"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "a call with a string argument inside a hole",
        source: r#"fn string Exclaim(string s) { return s + "!"; }
                   fn bool F() { return $"{Exclaim("x")}" == "x!"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "interpolation holes evaluated left to right",
        source: r#"behavior Recorder {
                       int trace = 0;
                       int Mark(int digit) { trace = trace * 10 + digit; return digit; }
                       int Probe() {
                           string s = $"{Mark(2)}{Mark(1)}{Mark(3)}";
                           return s == "213" ? trace : -1;
                       }
                   }"#,
        entry: "Recorder.Probe",
        args: &[],
        expect: Expect::Runs(Value::Int(213)),
    },
    Row {
        construct: "an interpolation with no hole",
        source: r#"fn bool F() { return $"abc" == "abc"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Runs(Value::Bool(true)),
    },
    Row {
        construct: "a lone `}` in an interpolation",
        source: r#"fn string F() { return $"a } b"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Unparsed("a `}` in an interpolated string is written `}}`"),
    },
    Row {
        construct: "an unparenthesised ternary in a hole",
        source: r#"fn string F(bool a, int b, int c) { return $"{a ? b : c}"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Unparsed("write `(a ? b : c)` inside `{}`"),
    },
    Row {
        construct: "an array in a hole",
        source: r#"fn string F() { int[] xs = [1]; return $"{xs}"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Rejected("shows numbers, text, flags, engine values and entities"),
    },
    Row {
        construct: "a struct in a hole",
        source: r#"struct Loot { int value; }
                   fn string F() { Loot l = Loot { value: 1 }; return $"{l}"; }"#,
        entry: "F",
        args: &[],
        expect: Expect::Rejected("shows numbers, text, flags, engine values and entities"),
    },
];

/// What a spec will change. Each row states the behaviour the spec must
/// deliver, and names the spec.
const PENDING: &[(&str, Row)] = &[];

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
        | (Expect::Rejected(fragment), Outcome::Rejected(messages))
        | (Expect::Unparsed(fragment), Outcome::Unparsed(messages)) => {
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
        host = Host::new()
            .with_fields(fresh_store(program, behavior))
            .for_entity(SUBJECT);
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
