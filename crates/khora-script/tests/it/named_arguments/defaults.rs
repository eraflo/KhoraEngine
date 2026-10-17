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

//! A parameter's default, `Type name = expr`, and where one is refused.
//!
//! A default is a constant — the rule a field's default follows — of the
//! parameter's type, and a parameter with one is followed only by parameters
//! with one. A method the engine calls, a handler and an event take none: the
//! engine gives every argument, a handler's parameters are its event's
//! payload, and a payload is raised whole.

use khora_script::vm::Value;

use super::{build, compile, errors, refused_at, run_fn, Instance};

// ─── What a default may be ──────────────────────────────────────────────────

/// **A constant, as a field's default is.** A number, a negated one, text, a
/// duration, an angle, an engine value of numbers: each is taken when the
/// parameter is left out.
#[test]
fn a_constant_default_of_every_kind_is_accepted_and_taken() {
    let program = build(
        r#"fn int Count(int n = -4) { return n; }
           fn string Label(string text = "idle") { return text; }
           fn float Wait(Duration d = 2s) { return d.Seconds; }
           fn float Turn(Angle a = 90deg) { return a.Radians; }
           fn float Along(Vec3 v = Vec3(1.0, 2.0, 3.0)) { return v.y; }
           fn bool Flag(bool on = true) { return on; }
           fn int CountDefault() { return Count(); }
           fn bool LabelDefault() { return Label() == "idle"; }
           fn float WaitDefault() { return Wait(); }
           fn bool TurnDefault() { float r = Turn(); return r > 1.57 && r < 1.58; }
           fn float AlongDefault() { return Along(); }
           fn bool FlagDefault() { return Flag(); }"#,
    );

    assert_eq!(run_fn(&program, "CountDefault", &[]), Value::Int(-4));
    assert_eq!(run_fn(&program, "LabelDefault", &[]), Value::Bool(true));
    assert_eq!(run_fn(&program, "WaitDefault", &[]), Value::Float(2.0));
    assert_eq!(run_fn(&program, "TurnDefault", &[]), Value::Bool(true));
    assert_eq!(run_fn(&program, "AlongDefault", &[]), Value::Float(2.0));
    assert_eq!(run_fn(&program, "FlagDefault", &[]), Value::Bool(true));
}

/// **Computed is refused**, with the message a field's default gets: a call
/// or a parameter read is not data.
#[test]
fn a_default_that_is_not_a_constant_is_refused_at_it() {
    let calling = "fn int F(int x = Base()) { return x; }
                   fn int Base() { return 1; }";
    refused_at(calling, "Base()", &["default must be a constant"]);

    let reading = "fn int F(int x, int y = x) { return y; }";
    refused_at(reading, "int y = x", &["default must be a constant"]);
}

/// **Of the parameter's type.** Text for an `int` is refused at the default.
#[test]
fn a_default_of_the_wrong_type_is_refused_at_it() {
    let source = r#"fn int F(int x = "fast") { return x; }"#;
    refused_at(source, r#""fast""#, &["expected `int`, found `string`"]);
}

/// **Defaults come last.** A parameter without one after a parameter with one
/// is refused at the later parameter, naming the earlier.
#[test]
fn a_parameter_without_a_default_after_one_with_a_default_is_refused() {
    let source = "fn int F(int x = 1, int y) { return x + y; }";
    refused_at(
        source,
        "int y",
        &["`y` needs a default: it follows `x`, which has one"],
    );
}

// ─── Who may have one ───────────────────────────────────────────────────────

/// A behavior method and a state method may: an ordinary call leaves the
/// parameter out and gets the default.
#[test]
fn a_behavior_method_and_a_state_method_may_have_defaults() {
    let program = build(
        "behavior Guard {
             int Reach(int base = 2) { return base; }
             int Own() { return Reach(); }
             state Patrol {
                 int Lap(int laps = 3) { return laps; }
                 int Inner() { return Lap(); }
             }
         }",
    );

    let mut guard = Instance::of(&program, "Guard");
    assert_eq!(guard.call("Own"), Value::Int(2));
    assert_eq!(guard.call("Inner"), Value::Int(3));
}

/// **A lifecycle method takes none**: the engine calls it, and gives every
/// argument. `Update`, `FixedUpdate` and `OnResumeFailed` each refuse one, at
/// the parameter.
#[test]
fn a_lifecycle_method_parameter_cannot_have_a_default() {
    for (method, parameter) in [
        ("void Update(float dt = 0.016) { }", "float dt = 0.016"),
        ("void FixedUpdate(float dt = 0.02) { }", "float dt = 0.02"),
        (
            r#"void OnResumeFailed(string member = "none") { }"#,
            r#"string member = "none""#,
        ),
    ] {
        let source = format!("behavior Guard {{ {method} }}");
        refused_at(
            &source,
            parameter,
            &["a lifecycle method is called by the engine, which gives every argument"],
        );
    }
}

/// **A handler takes none**: its parameters are what the event carries.
#[test]
fn a_handler_parameter_cannot_have_a_default() {
    let source = "event Damaged(int amount);
                  behavior Guard {
                      int health = 10;
                      on Damaged(int amount = 1) { health -= amount; }
                  }";
    refused_at(
        source,
        "int amount = 1",
        &["a handler's parameters are its event's payload"],
    );
}

/// **An event takes none**: a raise gives its whole payload.
#[test]
fn an_event_parameter_cannot_have_a_default() {
    let source = "event Damaged(int amount = 1);
                  behavior Guard { void Hurt() { this.Raise(Damaged(2)); } }";
    refused_at(
        source,
        "int amount = 1",
        &["an event's payload is given whole when raised"],
    );
}

/// **An operator's parameter is held to the rule too.** An operator is called
/// by its operator, which gives both operands, so a default there is never
/// passed — and one that is not even a constant must not slip through
/// unchecked.
#[test]
fn an_operator_parameter_default_is_not_accepted_unchecked() {
    let source = "struct Health {
                      int current;
                      static Health operator -(Health h, int damage = Nope()) { return h; }
                  }";
    refused_at(source, "int damage = Nope()", &[]);
}

// ─── What a default is built from ───────────────────────────────────────────

/// **A default written with names means what its names say.** `Vec3(z: 3.0,
/// x: 1.0, y: 2.0)` as a parameter's default is the vector `(1, 2, 3)` — what
/// it is anywhere else — not its values taken in the order written.
#[test]
fn a_default_constructed_with_named_arguments_builds_the_value_its_names_say() {
    let program = build(
        "fn float Digits(Vec3 v) { return v.x * 100.0 + v.y * 10.0 + v.z; }
         fn float Aim(Vec3 v = Vec3(z: 3.0, x: 1.0, y: 2.0)) { return Digits(v); }
         fn float Defaulted() { return Aim(); }
         fn float Written() { return Aim(Vec3(z: 3.0, x: 1.0, y: 2.0)); }",
    );

    assert_eq!(run_fn(&program, "Written", &[]), Value::Float(123.0));
    assert_eq!(
        run_fn(&program, "Defaulted", &[]),
        Value::Float(123.0),
        "the default is `x = 1`, `y = 2`, `z = 3`"
    );
}

/// **A struct declared below its use.** A default `P { y: 5 }` of a struct the
/// file declares after the function is accepted, as it is when the struct
/// comes first — a file's order decides nothing else a function may use.
#[test]
fn a_default_of_a_struct_declared_below_the_function_is_accepted() {
    let program = build(
        "fn int F(P p = P { y: 5 }) { return p.x * 10 + p.y; }
         fn int G() { return F(); }
         struct P { int x = 1; int y = 2; }",
    );

    assert_eq!(run_fn(&program, "G", &[]), Value::Int(15));
}

/// **A default means the same at every call.** `Vec3(1.0, 2.0, 3.0)` and
/// `Color(…)` written as defaults are the engine's constructors, as they are
/// where the function is declared — even when the call is made from a
/// behavior with a method of that name. Called from there, the default is
/// neither refused against the method's parameters nor run as the method.
#[test]
fn a_default_means_the_same_whichever_scope_it_is_passed_from() {
    // The method takes nothing: the default is not a call to it.
    let program = build(
        "fn float Green(Color c = Color(1.0, 0.5, 0.0, 1.0)) { return c.g; }
         fn float Free() { return Green(); }
         behavior Lamp {
             int Color() { return 3; }
             float Go() { return Green(); }
         }",
    );
    assert_eq!(run_fn(&program, "Free", &[]), Value::Float(0.5));
    assert_eq!(Instance::of(&program, "Lamp").call("Go"), Value::Float(0.5));

    // The method takes what the constructor takes: the default is still not
    // a call to it.
    let program = build(
        "fn float Digits(Vec3 v) { return v.x * 100.0 + v.y * 10.0 + v.z; }
         fn float Aim(Vec3 v = Vec3(1.0, 2.0, 3.0)) { return Digits(v); }
         fn float Free() { return Aim(); }
         behavior Turret {
             Vec3 nine = Vec3(9.0, 9.0, 9.0);
             Vec3 Vec3(float x, float y, float z) { return nine; }
             float Go() { return Aim(); }
         }",
    );
    assert_eq!(run_fn(&program, "Free", &[]), Value::Float(123.0));
    assert_eq!(
        Instance::of(&program, "Turret").call("Go"),
        Value::Float(123.0),
        "the default is the engine's `Vec3(1, 2, 3)`, not `Turret.Vec3`"
    );
}

/// **A wrong default is reported once**, at the default — not again at every
/// call that leaves it out.
#[test]
fn a_wrong_default_is_reported_once_however_many_calls_leave_it_out() {
    for (declarations, parameter) in [
        ("struct P { int x = 1; }", "P p = P { q: 1 }"),
        ("struct P { int x = 1; }", "P p = P { x: 1, x: 2 }"),
        ("struct S { Vec3 v; }", "S s = S { }"),
        ("", "int[] xs = [1, true]"),
    ] {
        let source = format!(
            "{declarations}
             fn int F({parameter}) {{ return 0; }}
             fn int A() {{ return F(); }}
             fn int B() {{ return F() + F(); }}"
        );
        let result = compile(&source);
        let found: Vec<&str> = errors(&result)
            .into_iter()
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(
            found.len(),
            1,
            "`{parameter}`, left out by three calls: {found:?}"
        );
    }
}
