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

//! A machine written down in the engine's terms and read back.
//!
//! What a scene keeps of a suspended machine is a [`FrozenMachine`]: frames
//! naming their function, registers holding a literal's text rather than its
//! slot. Freezing then thawing must hand back a machine that finishes exactly
//! as the original would — and a frozen machine that does not fit the program
//! it is thawed into must be refused, never run and never a panic.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{FrozenMachine, FrozenValue, PendingBody};
use khora_script::arena::{Object, Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, StrRef};
use khora_script::{
    check, compile, lex, parse, Function, Host, Instruction, Machine, Program, Run, Suspension,
    Value,
};

// ─── Programs ───────────────────────────────────────────────────────────────

fn function(name: &str, arity: usize, registers: usize, code: Vec<Instruction>) -> Function {
    Function {
        name: name.to_owned(),
        arity,
        registers,
        code,
        fingerprint: 0,
        sites: Vec::new(),
    }
}

/// `Main() = Double(21)`, where `Double(n)` yields once before `n * 2`.
fn main_code(double_at: usize) -> Vec<Instruction> {
    vec![
        Instruction::LoadConst {
            dst: 1,
            value: Value::Int(21),
        },
        Instruction::Call {
            function: double_at,
            base: 1,
            argc: 1,
            dst: 0,
        },
        Instruction::Return { src: 0 },
    ]
}

fn double_code() -> Vec<Instruction> {
    vec![
        Instruction::LoadConst {
            dst: 1,
            value: Value::Int(2),
        },
        Instruction::Yield,
        Instruction::MulInt {
            dst: 2,
            lhs: 0,
            rhs: 1,
        },
        Instruction::Return { src: 2 },
    ]
}

fn pausing_call() -> Program {
    Program {
        max_overdraft: 0,
        strings: Vec::new(),
        behaviors: Vec::new(),
        functions: vec![
            function("Main", 0, 3, main_code(1)),
            function("Double", 1, 3, double_code()),
        ],
    }
}

/// [`pausing_call`] with its two functions laid out the other way round.
fn pausing_call_reordered() -> Program {
    Program {
        max_overdraft: 0,
        strings: Vec::new(),
        behaviors: Vec::new(),
        functions: vec![
            function("Double", 1, 3, double_code()),
            function("Main", 0, 3, main_code(0)),
        ],
    }
}

/// [`pausing_call`] whose callee is called something else.
fn pausing_call_renamed() -> Program {
    Program {
        max_overdraft: 0,
        strings: Vec::new(),
        behaviors: Vec::new(),
        functions: vec![
            function("Main", 0, 3, main_code(1)),
            function("Triple", 1, 3, double_code()),
        ],
    }
}

/// Loads the literal at `index` of `strings`, yields, and returns it.
fn pausing_literal(strings: &[&str], index: u32) -> Program {
    Program {
        max_overdraft: 0,
        strings: strings.iter().map(|s| (*s).to_owned()).collect(),
        behaviors: Vec::new(),
        functions: vec![function(
            "Say",
            0,
            1,
            vec![
                Instruction::LoadStr { dst: 0, index },
                Instruction::Yield,
                Instruction::Return { src: 0 },
            ],
        )],
    }
}

/// Joins two literals into new text, then yields holding it.
fn pausing_joined() -> Program {
    Program {
        max_overdraft: 0,
        strings: vec!["wind".to_owned(), "-up".to_owned()],
        behaviors: Vec::new(),
        functions: vec![function(
            "Join",
            0,
            3,
            vec![
                Instruction::LoadStr { dst: 0, index: 0 },
                Instruction::LoadStr { dst: 1, index: 1 },
                Instruction::Concat {
                    dst: 2,
                    lhs: 0,
                    rhs: 1,
                },
                Instruction::Yield,
                Instruction::Return { src: 2 },
            ],
        )],
    }
}

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

/// Runs `machine` until it stops, and says why.
fn run_until_stopped(machine: &mut Machine, program: &Program, host: &mut Host) -> Run {
    machine.run(program, host, u64::MAX)
}

/// Resumes `machine` until it completes, treating every `await` as elapsed.
fn finish(machine: &mut Machine, program: &Program, host: &mut Host) -> Value {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return machine.result(),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// [`pausing_call`] stopped at its yield, two calls deep.
fn stopped_inside_the_call() -> (Program, Machine) {
    let program = pausing_call();
    let mut machine = Machine::new(&program, "Main", &[]).expect("entry exists");
    assert_eq!(
        run_until_stopped(&mut machine, &program, &mut Host::new()),
        Run::Suspended(Suspension::Awaiting)
    );
    assert_eq!(machine.depth(), 2, "stopped inside `Double`");
    (program, machine)
}

fn frozen_inside_the_call() -> FrozenMachine {
    let (program, machine) = stopped_inside_the_call();
    machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes")
}

// ─── A machine and its frozen form ─────────────────────────────────────────

/// **The round trip is lossless.** Thawed into the program it was frozen in,
/// a machine is the machine it was — and in particular not a finished one.
#[test]
fn a_machine_frozen_mid_call_thaws_to_the_machine_it_was() {
    let (program, machine) = stopped_inside_the_call();
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");

    let thawed = Machine::thaw(&frozen, &program).expect("thaws into its own program");

    assert!(!thawed.is_finished());
    assert_eq!(thawed.depth(), machine.depth());
    assert_eq!(thawed.program_counter(), machine.program_counter());
    assert_eq!(thawed, machine);
}

/// Resuming the thawed machine lands where resuming the original does.
#[test]
fn a_thawed_machine_finishes_as_the_original_does() {
    let (program, mut original) = stopped_inside_the_call();
    let frozen = original
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    let mut thawed = Machine::thaw(&frozen, &program).expect("thaws");

    assert_eq!(
        finish(&mut original, &program, &mut Host::new()),
        Value::Int(42)
    );
    assert_eq!(
        finish(&mut thawed, &program, &mut Host::new()),
        Value::Int(42)
    );
}

/// **The VM's invariant, through a save.** A fuel cut can land on any
/// instruction boundary; writing the machine down and reading it back at every
/// one of them must still give the uninterrupted result.
#[test]
fn freezing_at_every_fuel_cut_changes_nothing() {
    let program = build(
        "fn int Double(int n) { return n * 2; }
         fn int F() {
             int total = 0;
             for (int i = 0; i < 5; i = i + 1) { total = total + Double(i); }
             return total;
         }",
    );

    for slice in 1..=12u64 {
        let mut machine = Machine::new(&program, "F", &[]).expect("entry exists");
        let mut host = Host::new();
        let mut cuts = 0;
        loop {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    cuts += 1;
                    assert!(cuts < 10_000, "slice {slice} is not making progress");
                    let frozen = machine
                        .freeze(&program, PendingBody::Update)
                        .unwrap_or_else(|| panic!("slice {slice}: cut {cuts} did not freeze"));
                    machine = Machine::thaw(&frozen, &program)
                        .unwrap_or_else(|| panic!("slice {slice}: cut {cuts} did not thaw"));
                }
                other => panic!("slice {slice}: unexpected {other:?}"),
            }
        }
        assert_eq!(
            machine.result(),
            Value::Int(20),
            "a slice of {slice}, frozen at every cut, diverged"
        );
    }
}

/// A frame names its function, outermost first — what a reader of a save can
/// check against the script.
#[test]
fn a_frozen_frame_names_its_function() {
    let frozen = frozen_inside_the_call();

    let names: Vec<&str> = frozen
        .frames
        .iter()
        .map(|frame| frame.function.as_str())
        .collect();
    assert_eq!(names, ["Main", "Double"]);
}

/// What finishing the machine owes is whatever the caller said, untouched.
#[test]
fn the_body_given_to_freeze_is_the_body_the_frozen_machine_holds() {
    let (program, machine) = stopped_inside_the_call();

    for body in [
        PendingBody::Sequence,
        PendingBody::Spawn,
        PendingBody::Update,
        PendingBody::Timer {
            timer: "Guard.__every(0.5)".to_owned(),
            rearm: FrozenValue::Float(0.5),
        },
        PendingBody::Timer {
            timer: "Guard.Patrol.__after(2)".to_owned(),
            rearm: FrozenValue::Null,
        },
    ] {
        let frozen = machine.freeze(&program, body.clone()).expect("freezes");
        assert_eq!(frozen.body, body);
    }
}

/// **Named, not numbered.** The same functions laid out in another order are
/// the same program to a frozen machine: each frame finds its function by
/// name, and the machine finishes as it would have.
#[test]
fn a_thawed_frame_finds_its_function_by_name_not_by_position() {
    let frozen = frozen_inside_the_call();
    let reordered = pausing_call_reordered();

    let mut thawed = Machine::thaw(&frozen, &reordered).expect("both functions are there");

    assert_eq!(
        finish(&mut thawed, &reordered, &mut Host::new()),
        Value::Int(42)
    );
}

/// A frame whose function the program no longer has is refused — resuming it
/// would run whatever now sits at that position.
#[test]
fn thawing_into_a_program_without_the_function_gives_none() {
    let frozen = frozen_inside_the_call();

    assert!(Machine::thaw(&frozen, &pausing_call_renamed()).is_none());
}

// ─── Text ───────────────────────────────────────────────────────────────────

/// A literal register is written as its text, and read back as the program's
/// literal with that text — wherever the program keeps it.
#[test]
fn a_literal_register_round_trips_to_the_same_text() {
    let program = pausing_literal(&["hello"], 0);
    let mut machine = Machine::new(&program, "Say", &[]).expect("entry exists");
    assert_eq!(
        run_until_stopped(&mut machine, &program, &mut Host::new()),
        Run::Suspended(Suspension::Awaiting)
    );

    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    assert_eq!(
        frozen.registers[0],
        FrozenValue::Literal("hello".to_owned())
    );

    let host = Host::new();
    let thawed = Machine::thaw(&frozen, &program).expect("thaws");
    let register = thawed.register(0).expect("the register exists");
    assert_eq!(thawed.resolve_str(register, &program, &host), Ok("hello"));

    // The same literal at another position of another table.
    let moved = pausing_literal(&["other", "hello"], 1);
    let thawed = Machine::thaw(&frozen, &moved).expect("the text is there");
    let register = thawed.register(0).expect("the register exists");
    assert_eq!(thawed.resolve_str(register, &moved, &host), Ok("hello"));
}

/// A literal the program no longer has cannot be read back as anything.
#[test]
fn a_literal_the_program_lacks_does_not_thaw() {
    let program = pausing_literal(&["hello"], 0);
    let mut machine = Machine::new(&program, "Say", &[]).expect("entry exists");
    run_until_stopped(&mut machine, &program, &mut Host::new());
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");

    assert!(Machine::thaw(&frozen, &pausing_literal(&["other"], 0)).is_none());
    assert_eq!(
        Value::thaw(&FrozenValue::Literal("gone".to_owned()), &program),
        None
    );
}

/// A string register naming a slot outside the program's table is not a
/// literal of that program, and does not freeze as one.
#[test]
fn a_literal_outside_the_table_does_not_freeze() {
    let program = pausing_literal(&["hello"], 0);

    assert_eq!(
        Value::Str(StrRef::Const(0)).freeze(&program),
        Some(FrozenValue::Literal("hello".to_owned()))
    );
    assert_eq!(Value::Str(StrRef::Const(9)).freeze(&program), None);
}

/// **Text built while running expires.** It freezes as expired, and the
/// thawed register fails to resolve the way expired text does — against an
/// empty arena and against one holding someone else's text alike.
#[test]
fn text_built_while_running_freezes_as_expired_and_never_resolves() {
    let program = pausing_joined();
    let mut machine = Machine::new(&program, "Join", &[]).expect("entry exists");
    let mut host = Host::new();
    assert_eq!(
        run_until_stopped(&mut machine, &program, &mut host),
        Run::Suspended(Suspension::Awaiting)
    );
    let built = machine.register(2).expect("the register exists");
    assert_eq!(
        machine.resolve_str(built, &program, &host),
        Ok("wind-up"),
        "the text exists before the save"
    );

    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    assert_eq!(frozen.registers[2], FrozenValue::Expired);
    assert_eq!(
        frozen.registers[0],
        FrozenValue::Literal("wind".to_owned()),
        "a literal beside it is kept"
    );

    let thawed = Machine::thaw(&frozen, &program).expect("thaws");
    let register = thawed.register(2).expect("the register exists");

    assert_eq!(
        thawed.resolve_str(register, &program, &Host::new()),
        Err(Fault::BadString),
        "an empty arena"
    );

    let mut busy = Host::new();
    for text in ["someone", "else's", "text"] {
        busy.arena
            .alloc(Object::Str(text.to_owned()))
            .expect("the arena has room");
    }
    assert_eq!(
        thawed.resolve_str(register, &program, &busy),
        Err(Fault::BadString),
        "an arena holding other text"
    );

    let expired = Value::thaw(&FrozenValue::Expired, &program).expect("expired text thaws");
    assert_eq!(
        thawed.resolve_str(expired, &program, &busy),
        Err(Fault::BadString)
    );
}

/// Every register that is not text is written as itself and read back as
/// itself.
#[test]
fn every_scalar_register_survives_freezing() {
    let program = pausing_call();
    let entity = EntityId {
        index: 4,
        generation: 2,
    };

    for (value, frozen) in [
        (Value::Unit, FrozenValue::Unit),
        (Value::Int(-7), FrozenValue::Int(-7)),
        (Value::Float(0.25), FrozenValue::Float(0.25)),
        (Value::Bool(true), FrozenValue::Bool(true)),
        (Value::Entity(entity), FrozenValue::Entity(entity)),
        (
            Value::Vec2(khora_core::math::Vec2::new(1.0, 2.0)),
            FrozenValue::Vec2(khora_core::math::Vec2::new(1.0, 2.0)),
        ),
        (
            Value::Vec3(khora_core::math::Vec3::new(1.0, 2.0, 3.0)),
            FrozenValue::Vec3(khora_core::math::Vec3::new(1.0, 2.0, 3.0)),
        ),
        (
            Value::Vec4(khora_core::math::Vec4::new(1.0, 2.0, 3.0, 4.0)),
            FrozenValue::Vec4(khora_core::math::Vec4::new(1.0, 2.0, 3.0, 4.0)),
        ),
        (
            Value::Quat(khora_core::math::Quaternion::IDENTITY),
            FrozenValue::Quat(khora_core::math::Quaternion::IDENTITY),
        ),
        (
            Value::Color(khora_core::math::LinearRgba::new(0.5, 0.25, 1.0, 1.0)),
            FrozenValue::Color(khora_core::math::LinearRgba::new(0.5, 0.25, 1.0, 1.0)),
        ),
        (Value::Null, FrozenValue::Null),
    ] {
        assert_eq!(value.freeze(&program), Some(frozen.clone()), "{value:?}");
        assert_eq!(Value::thaw(&frozen, &program), Some(value), "{frozen:?}");
    }
}

// ─── What does not fit ─────────────────────────────────────────────────────

/// **Refused, never run and never a panic.** A save is input: edited by hand,
/// written by another build, or damaged. Each shape below cannot be a machine
/// of this program.
#[test]
fn a_malformed_frozen_machine_does_not_thaw() {
    let program = pausing_call();
    let valid = frozen_inside_the_call();
    assert!(
        Machine::thaw(&valid, &program).is_some(),
        "the unaltered machine thaws"
    );

    let mut cases: Vec<(&str, FrozenMachine)> = Vec::new();

    let mut no_frames = valid.clone();
    no_frames.frames.clear();
    cases.push(("no frames at all", no_frames));

    let mut base_beyond = valid.clone();
    base_beyond.frames[1].base = 1_000;
    cases.push(("a frame starting past the register file", base_beyond));

    let mut outer_base_beyond = valid.clone();
    outer_base_beyond.frames[0].base = u64::MAX;
    cases.push((
        "the outer frame starting past the register file",
        outer_base_beyond,
    ));

    let mut result_beyond = valid.clone();
    result_beyond.frames[1].result = 1_000;
    cases.push(("a result register outside the register file", result_beyond));

    let mut too_few_registers = valid.clone();
    too_few_registers.registers.truncate(1);
    cases.push((
        "frames that do not fit the register file",
        too_few_registers,
    ));

    let mut no_registers = valid.clone();
    no_registers.registers.clear();
    cases.push(("no registers at all", no_registers));

    let mut unknown_function = valid.clone();
    unknown_function.frames[0].function = "Nowhere".to_owned();
    cases.push(("a function the program lacks", unknown_function));

    let mut unknown_literal = valid.clone();
    unknown_literal.registers[0] = FrozenValue::Literal("missing".to_owned());
    cases.push(("a literal the program lacks", unknown_literal));

    for (what, frozen) in cases {
        assert!(
            Machine::thaw(&frozen, &program).is_none(),
            "{what}: thawed anyway"
        );
    }
}

// ─── A behavior mid-`await` ────────────────────────────────────────────────

const ATTACKER: &str = "behavior Attacker {
                            int landed = 0;
                            async void Attack() {
                                int blow = 41;
                                await 0.5s;
                                landed = blow + 1;
                            }
                        }";

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

/// An instance of `Attacker` with its declared defaults in place.
fn attacker_host(program: &Program) -> Host {
    let layout = program.layout("Attacker").expect("the behavior exists");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(subject());
    let mut init =
        Machine::new(program, &init_name("Attacker"), &[]).expect("the behavior has defaults");
    finish(&mut init, program, &mut host);
    host
}

/// **The point of it all.** A member stopped at its `await`, written down,
/// read back in a program compiled again from the same source and a host
/// holding only the saved fields, lands the same blow.
#[test]
fn a_member_frozen_at_its_await_resumes_in_a_fresh_host() {
    let program = build(ATTACKER);
    let mut host = attacker_host(&program);
    let attack = resolve_member(&program, "Attacker", "Attack", &host).expect("the member exists");

    let mut original = Machine::new(&program, &attack, &[]).expect("entry exists");
    assert_eq!(
        run_until_stopped(&mut original, &program, &mut host),
        Run::Suspended(Suspension::Awaiting)
    );
    host.awaiting = None;
    let saved_fields = host.fields.clone();

    let frozen = original
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    assert_eq!(frozen.frames.len(), 1);
    assert_eq!(
        frozen.frames[0].function, attack,
        "the frame names `Attack`"
    );
    let frozen: FrozenMachine =
        serde_json::from_str(&serde_json::to_string(&frozen).expect("serialises")).expect("parses");

    // The original, left running.
    finish(&mut original, &program, &mut host);
    let expected = host.fields.get(0).cloned();
    assert_eq!(expected, Some(Persisted::Scalar(Value::Int(42))));

    // The save, loaded elsewhere.
    let reloaded = build(ATTACKER);
    let mut fresh = Host::new().with_fields(saved_fields).for_entity(subject());
    let mut thawed = Machine::thaw(&frozen, &reloaded).expect("thaws into the same source");
    finish(&mut thawed, &reloaded, &mut fresh);

    assert_eq!(fresh.fields.get(0).cloned(), expected);
}
