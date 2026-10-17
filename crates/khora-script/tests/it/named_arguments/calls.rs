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

//! What a labelled call does, for every kind of callee: written in any order,
//! it means what the positional call means; an omitted parameter with a
//! default takes the default; the arguments run in the order written.

use khora_core::script::ScriptValue;
use khora_script::vm::Value;

use super::{build, run_fn, Instance, SUBJECT};

// ─── A script `fn` ──────────────────────────────────────────────────────────

/// **Any order means the same call.** Named in declaration order, reversed,
/// or after a positional one, `Pair` receives `a = 3`, `b = 4` — the value
/// the positional call returns.
#[test]
fn a_function_called_with_names_in_any_order_receives_what_a_positional_call_does() {
    let program = build(
        "fn int Pair(int a, int b) { return a * 10 + b; }
         fn int Positional() { return Pair(3, 4); }
         fn int Named() { return Pair(a: 3, b: 4); }
         fn int Reversed() { return Pair(b: 4, a: 3); }
         fn int Mixed() { return Pair(3, b: 4); }",
    );

    for caller in ["Positional", "Named", "Reversed", "Mixed"] {
        assert_eq!(
            run_fn(&program, caller, &[]),
            Value::Int(34),
            "`{caller}` gives `a` 3 and `b` 4"
        );
    }
}

/// **An omitted parameter takes its default.** A trailing default left out, a
/// middle one skipped by naming the one after it, and both given by position:
/// each parameter holds what was written, or its default.
#[test]
fn a_function_parameter_left_out_takes_its_default() {
    let program = build(
        "fn int Move(int x, int speed = 3, int turns = 5) {
             return x * 100 + speed * 10 + turns;
         }
         fn int AllDefaults() { return Move(1); }
         fn int MiddleSkipped() { return Move(1, turns: 7); }
         fn int ByPosition() { return Move(1, 2); }
         fn int AllNamed() { return Move(turns: 9, x: 2, speed: 4); }",
    );

    for (caller, expected) in [
        ("AllDefaults", 135),
        ("MiddleSkipped", 137),
        ("ByPosition", 125),
        ("AllNamed", 249),
    ] {
        assert_eq!(
            run_fn(&program, caller, &[]),
            Value::Int(expected),
            "`{caller}`"
        );
    }
}

/// A named `int` given to a `float` parameter is widened, as a positional one
/// is.
#[test]
fn a_named_int_argument_is_widened_to_a_float_parameter() {
    let program = build(
        "fn float Scale(float by, float of) { return by * of; }
         fn float Positional() { return Scale(2, 1.5); }
         fn float Named() { return Scale(of: 1.5, by: 2); }",
    );

    assert_eq!(run_fn(&program, "Positional", &[]), Value::Float(3.0));
    assert_eq!(run_fn(&program, "Named", &[]), Value::Float(3.0));
}

// ─── A behavior method, a state method ──────────────────────────────────────

/// A behavior's own method takes names and defaults as a free function does.
#[test]
fn a_behavior_method_takes_names_in_any_order_and_its_defaults() {
    let program = build(
        "behavior Car {
             int Score(int laps, int bonus = 2) { return laps * 10 + bonus; }
             int Positional() { return Score(4, 1); }
             int Reversed() { return Score(bonus: 1, laps: 4); }
             int Defaulted() { return Score(laps: 4); }
             int DefaultedByPosition() { return Score(4); }
         }",
    );

    for (member, expected) in [
        ("Positional", 41),
        ("Reversed", 41),
        ("Defaulted", 42),
        ("DefaultedByPosition", 42),
    ] {
        let mut car = Instance::of(&program, "Car");
        assert_eq!(car.call(member), Value::Int(expected), "`{member}`");
    }
}

/// A state's method, called from inside the state, takes names and defaults.
#[test]
fn a_state_method_takes_names_in_any_order_and_its_defaults() {
    let program = build(
        "behavior Door {
             state Closed {
                 int Swing(int force, int angle = 5) { return force * 10 + angle; }
                 int Positional() { return Swing(3, 1); }
                 int Reversed() { return Swing(angle: 1, force: 3); }
                 int Defaulted() { return Swing(force: 3); }
             }
         }",
    );

    for (member, expected) in [("Positional", 31), ("Reversed", 31), ("Defaulted", 35)] {
        let mut door = Instance::of(&program, "Door");
        assert_eq!(door.call(member), Value::Int(expected), "`{member}`");
    }
}

// ─── An engine function, an engine constructor ──────────────────────────────

/// **An engine function is named by its Rust parameters.** `Lerp(from, to,
/// t)` and `Clamp(value, low, high)` take their names in any order, and a
/// named `int` is widened to the `float` they take.
#[test]
fn an_engine_function_takes_its_rust_parameter_names_in_any_order() {
    let program = build(
        "fn float LerpPositional() { return Lerp(0.0, 8.0, 0.25); }
         fn float LerpReversed() { return Lerp(t: 0.25, to: 8.0, from: 0.0); }
         fn float LerpMixed() { return Lerp(0.0, t: 0.25, to: 8.0); }
         fn float ClampPositional() { return Clamp(15.0, 0.0, 10.0); }
         fn float ClampNamed() { return Clamp(high: 10.0, value: 15.0, low: 0.0); }
         fn float ClampWidened() { return Clamp(low: 0, high: 10, value: 15); }",
    );

    for (caller, expected) in [
        ("LerpPositional", 2.0),
        ("LerpReversed", 2.0),
        ("LerpMixed", 2.0),
        ("ClampPositional", 10.0),
        ("ClampNamed", 10.0),
        ("ClampWidened", 10.0),
    ] {
        assert_eq!(
            run_fn(&program, caller, &[]),
            Value::Float(expected),
            "`{caller}`"
        );
    }
}

/// **An engine constructor is named by its fields.** `Vec3(z:, x:, y:)` and
/// `Color(a:, b:, g:, r:)` build the value the positional constructor builds.
#[test]
fn an_engine_constructor_takes_its_field_names_in_any_order() {
    let program = build(
        "fn float Digits(Vec3 v) { return v.x * 100.0 + v.y * 10.0 + v.z; }
         fn float VecPositional() { return Digits(Vec3(1.0, 2.0, 3.0)); }
         fn float VecNamed() { return Digits(Vec3(z: 3.0, x: 1.0, y: 2.0)); }
         fn float VecMixed() { return Digits(Vec3(1.0, z: 3.0, y: 2.0)); }
         fn float ColorNamed() {
             Color c = Color(a: 4.0, b: 3.0, g: 2.0, r: 1.0);
             return c.r * 1000.0 + c.g * 100.0 + c.b * 10.0 + c.a;
         }",
    );

    for (caller, expected) in [
        ("VecPositional", 123.0),
        ("VecNamed", 123.0),
        ("VecMixed", 123.0),
        ("ColorNamed", 1234.0),
    ] {
        assert_eq!(
            run_fn(&program, caller, &[]),
            Value::Float(expected),
            "`{caller}`"
        );
    }
}

// ─── An event's payload, a state's data ─────────────────────────────────────

/// **A payload is named by the event's parameters.** Raised with its fields
/// named in reverse, or one by position and one by name, the event carries its
/// payload in declaration order — what the positional raise carries.
#[test]
fn an_event_payload_named_in_any_order_is_carried_in_declaration_order() {
    let program = build(
        "event Damaged(int amount, Entity source);
         behavior Guard {
             void Positional() { this.Raise(Damaged(3, this)); }
             void Reversed() { this.Raise(Damaged(source: this, amount: 3)); }
             void Mixed() { this.Raise(Damaged(3, source: this)); }
         }",
    );

    for member in ["Positional", "Reversed", "Mixed"] {
        let mut guard = Instance::of(&program, "Guard");
        guard.call(member);
        let raised = guard.raised();
        assert_eq!(raised.len(), 1, "`{member}` raises one event: {raised:?}");
        assert_eq!(raised[0].name, "Damaged");
        assert_eq!(
            raised[0].args,
            vec![ScriptValue::Int(3), ScriptValue::Entity(SUBJECT)],
            "`{member}` carries `amount` then `source`"
        );
    }
}

/// A named payload reaches the handler's parameters by their place in the
/// declaration.
#[test]
fn a_named_payload_reaches_the_handler() {
    let program = build(
        "event Struck(int damage, int times);
         behavior Guard {
             int health = 100;
             on Struck(int damage, int times) { health -= damage * times; }
             void Hurt() { this.Raise(Struck(times: 2, damage: 7)); }
             int Health() { return health; }
         }",
    );

    let mut guard = Instance::of(&program, "Guard");
    guard.call("Hurt");
    guard.deliver_raised();
    assert_eq!(guard.call("Health"), Value::Int(86), "7 damage, twice");
}

/// **`become` names the state's parameters.** Named in reverse, mixed, or by
/// position, the state is entered with `range = 2` and `prey = 7`.
#[test]
fn become_with_named_state_data_enters_the_state_as_a_positional_become_does() {
    let source = |transition: &str| {
        format!(
            "behavior Guard {{
                 state Patrol {{
                     void Spot() {{ {transition} }}
                     int Read() {{ return 0; }}
                 }}
                 state Chase(int range, int prey) {{
                     int Read() {{ return prey * 10 + range; }}
                 }}
             }}"
        )
    };

    for transition in [
        "become Chase(2, 7);",
        "become Chase(prey: 7, range: 2);",
        "become Chase(2, prey: 7);",
    ] {
        let program = build(&source(transition));
        let mut guard = Instance::of(&program, "Guard");
        guard.call("Spot");
        assert_eq!(guard.call("Read"), Value::Int(72), "`{transition}`");
    }
}

// ─── The order the arguments run in ─────────────────────────────────────────

/// **Written order, not declared order.** `Mark` appends its digit to
/// `trace`. Named against the parameters' order, `second: Mark(2)` runs before
/// `first: Mark(1)` — `trace` reads 21 — while `Pair` still receives `first =
/// 1`, `second = 2`.
#[test]
fn named_arguments_run_in_the_order_written() {
    let program = build(
        "behavior Recorder {
             int trace = 0;
             int Mark(int digit) { trace = trace * 10 + digit; return digit; }
             int Pair(int first, int second) { return first * 10 + second; }
             int Probe() {
                 int got = Pair(second: Mark(2), first: Mark(1));
                 return got;
             }
         }",
    );

    let mut recorder = Instance::of(&program, "Recorder");
    assert_eq!(
        recorder.call("Probe"),
        Value::Int(12),
        "`first` is 1 and `second` is 2"
    );
    assert_eq!(
        recorder.field("trace"),
        Value::Int(21),
        "`Mark(2)`, written first, ran first"
    );
}

/// The same holds for an engine function and an engine constructor: their
/// named arguments run left to right as written.
#[test]
fn named_arguments_of_an_engine_call_run_in_the_order_written() {
    let program = build(
        "behavior Recorder {
             int trace = 0;
             float Mark(int digit) { trace = trace * 10 + digit; return (float)digit; }
             float Native() { return Lerp(t: Mark(3), to: Mark(2), from: Mark(1)); }
             float Built() { Vec3 v = Vec3(z: Mark(3), y: Mark(2), x: Mark(1)); return v.x; }
         }",
    );

    let mut native = Instance::of(&program, "Recorder");
    native.call("Native");
    assert_eq!(native.field("trace"), Value::Int(321), "`Lerp`'s arguments");

    let mut built = Instance::of(&program, "Recorder");
    assert_eq!(built.call("Built"), Value::Float(1.0), "`x` is `Mark(1)`");
    assert_eq!(built.field("trace"), Value::Int(321), "`Vec3`'s arguments");
}
