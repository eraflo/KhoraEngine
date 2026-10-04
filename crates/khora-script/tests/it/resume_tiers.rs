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

//! A suspended body resumes in the code as it is now, in tiers.
//!
//! Exact when the program is the one it was frozen in; unchanged when every
//! function on its stack is; rebuilt at the same named sites when the code
//! around them moved; restarted from its entry with its original arguments
//! when only its member is left; abandoned when even that is gone.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{FrozenMachine, PendingBody};
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, Abandoned, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// `Guard`, with the same fields in every version, and `members`.
fn guard(members: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int starts = 0;
             int before = 0;
             int later = 0;
             int extra = 0;
             int landed = 0;
             {members}
         }}"
    ))
}

/// The attack every edit below starts from.
const ATTACK: &str = "async void Attack() {
                          starts += 1;
                          int blow = 41;
                          before += 1;
                          await 1.0s;
                          later += 1;
                          landed = blow + 1;
                      }";

/// A member the attack never reaches.
const IDLE: &str = "void Idle() { }";

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

/// Runs `machine` to the end, every `await` treated as elapsed.
fn finish(machine: &mut Machine, program: &Program, host: &mut Host) {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return,
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// A body of `program` stopped at an `await`, frozen, with the fields of the
/// instance it ran on.
struct Stopped {
    frozen: FrozenMachine,
    fingerprint: u64,
    fields: PersistentStore,
}

/// Starts `member` of `program` with `args` on a fresh instance and freezes it
/// at its first `await`.
fn stopped(program: &Program, member: &str, args: &[Value]) -> Stopped {
    let layout = program.layout("Guard").expect("the behavior has a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(subject());
    let mut init = Machine::new(program, &init_name("Guard"), &[]).expect("defaults exist");
    finish(&mut init, program, &mut host);

    let mut machine =
        Machine::new(program, &format!("Guard.{member}"), args).expect("the member exists");
    assert_eq!(
        machine.run(program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    host.awaiting = None;
    Stopped {
        frozen: machine
            .freeze(program, PendingBody::Sequence)
            .expect("a machine of this program freezes"),
        fingerprint: program.fingerprint(),
        fields: host.fields.clone(),
    }
}

/// Resumes `stopped` into `program`, runs it to the end on its instance, and
/// hands back the tier and the fields afterwards.
fn resumed_in(stopped: &Stopped, program: &Program) -> (ResumeTier, Fields) {
    let (mut machine, tier) = resume(&stopped.frozen, stopped.fingerprint, program)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    let mut host = Host::new()
        .with_fields(stopped.fields.clone())
        .for_entity(subject());
    finish(&mut machine, program, &mut host);
    (
        tier,
        Fields {
            program: program.clone(),
            store: host.fields,
        },
    )
}

/// An instance's fields, read by name.
struct Fields {
    program: Program,
    store: PersistentStore,
}

impl Fields {
    fn get(&self, name: &str) -> Option<i64> {
        let slot = self.program.layout("Guard")?.slot_of(name)?;
        match self.store.get(slot)? {
            Persisted::Scalar(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }

    /// `(starts, before, later, extra, landed)`.
    fn all(&self) -> [Option<i64>; 5] {
        ["starts", "before", "later", "extra", "landed"].map(|name| self.get(name))
    }
}

/// [`ATTACK`], stopped at its `await`.
fn attack_stopped() -> Stopped {
    stopped(&guard(&format!("{ATTACK}\n{IDLE}")), "Attack", &[])
}

// ─── Tiers 1 and 2: the code it stands in is the same ──────────────────────

/// **Exact.** The same source compiled again is the same program: the body
/// resumes as frozen and finishes once.
#[test]
fn an_unchanged_program_resumes_exactly() {
    let stopped = attack_stopped();
    let (tier, fields) = resumed_in(&stopped, &guard(&format!("{ATTACK}\n{IDLE}")));

    assert_eq!(tier, ResumeTier::Exact);
    assert_eq!(fields.all(), [Some(1), Some(1), Some(1), Some(0), Some(42)]);
}

/// **Unchanged.** Another member was edited: the program is a different one,
/// but every function on the body's stack is the same code.
#[test]
fn an_edit_in_another_function_resumes_unchanged() {
    let stopped = attack_stopped();
    let edited = guard(&format!("{ATTACK}\nvoid Idle() {{ extra += 1; }}"));
    assert_ne!(edited.fingerprint(), stopped.fingerprint, "the premise");

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Unchanged);
    assert_eq!(fields.all(), [Some(1), Some(1), Some(1), Some(0), Some(42)]);
}

// ─── Tier 3: rebuilt at the same site ───────────────────────────────────────

/// **Rebuilt.** The code after the `await` changed: the frame is rebuilt at
/// the same named site, `blow` carried by name, and the new statements run —
/// the ones before the `await` do not run again.
#[test]
fn an_edit_after_the_await_rebuilds_and_runs_the_new_code() {
    let stopped = attack_stopped();
    let edited = guard(&format!(
        "async void Attack() {{
             starts += 1;
             int blow = 41;
             before += 1;
             await 1.0s;
             later += 10;
             extra += 1;
             landed = blow + 2;
         }}
         {IDLE}"
    ));

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Rebuilt);
    assert_eq!(
        fields.all(),
        [Some(1), Some(1), Some(10), Some(1), Some(43)],
        "(starts, before, later, extra, landed)"
    );
}

/// A statement inserted before the `await` is not run: the frame stands past
/// it, at the same site it stood at before.
#[test]
fn inserting_a_statement_before_the_await_rebuilds_without_running_it() {
    let stopped = attack_stopped();
    let edited = guard(&format!(
        "{}\n{IDLE}",
        ATTACK.replace("before += 1;", "before += 1;\n extra += 1;")
    ));

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Rebuilt);
    assert_eq!(
        fields.all(),
        [Some(1), Some(1), Some(1), Some(0), Some(42)],
        "the inserted `extra += 1` did not run"
    );
}

/// **Two frames rebuilt.** The `await` is inside a callee; the caller was
/// edited after the call. Both frames come back at their sites, and the
/// callee's result lands in the caller's local.
#[test]
fn a_rebuilt_caller_receives_its_callees_result() {
    const WIND: &str = "async int Wind() {
                            int power = 5;
                            await 1.0s;
                            return power * 2;
                        }";
    let original = guard(&format!(
        "{WIND}
         async void Strike() {{
             starts += 1;
             int got = Wind();
             landed = got;
         }}"
    ));
    let edited = guard(&format!(
        "{WIND}
         async void Strike() {{
             starts += 1;
             int got = Wind();
             landed = got + 100;
             extra += 1;
         }}"
    ));
    let stopped = stopped(&original, "Strike", &[]);
    assert_eq!(stopped.frozen.frames.len(), 2, "stopped inside `Wind`");

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Rebuilt);
    assert_eq!(
        (
            fields.get("starts"),
            fields.get("landed"),
            fields.get("extra")
        ),
        (Some(1), Some(110), Some(1)),
        "the callee's 10 reached the caller's edited code"
    );
}

// ─── Tier 4: restarted from the member's entry ──────────────────────────────

/// A local renamed and nothing else is the same bytecode — locals are
/// registers — so the function is unchanged and the body resumes exactly.
#[test]
fn a_local_renamed_alone_resumes_exactly() {
    let stopped = attack_stopped();
    let edited = guard(&format!(
        "{}
{IDLE}",
        ATTACK
            .replace("int blow = 41;", "int hit = 41;")
            .replace("landed = blow + 1;", "landed = hit + 1;")
    ));

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Exact);
    assert_eq!(
        fields.all(),
        [Some(1), Some(1), Some(1), Some(0), Some(42)],
        "resumed as frozen: nothing ran twice"
    );
}

/// A local in scope at the site was renamed, in a function whose code also
/// changed: the frame cannot be rebuilt, so the body runs again from its
/// entry — and what it did before the cut happens again.
#[test]
fn a_local_renamed_before_the_await_restarts_the_body() {
    let stopped = attack_stopped();
    let edited = guard(&format!(
        "{}
{IDLE}",
        ATTACK.replace("int blow = 41;", "int hit = 41;").replace(
            "landed = blow + 1;",
            "landed = hit + 1;
 extra += 5;"
        )
    ));

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Restarted);
    assert_eq!(
        fields.all(),
        [Some(2), Some(2), Some(1), Some(5), Some(42)],
        "the body ran again from the top, in the edited code"
    );
}

/// A local whose type changed cannot carry its value across.
#[test]
fn a_local_whose_type_changed_restarts_the_body() {
    let stopped = attack_stopped();
    let edited = guard(&format!(
        "{}\n{IDLE}",
        ATTACK
            .replace("int blow = 41;", "float blow = 41.0;")
            .replace("landed = blow + 1;", "landed = 42;")
    ));

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Restarted);
    assert_eq!(fields.all(), [Some(2), Some(2), Some(1), Some(0), Some(42)]);
}

/// The `await` the body stood at is gone: there is no site to rebuild it at.
#[test]
fn deleting_the_await_restarts_the_body() {
    let stopped = attack_stopped();
    let edited = guard(&format!("{}\n{IDLE}", ATTACK.replace("await 1.0s;", "")));

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Restarted);
    assert_eq!(fields.all(), [Some(2), Some(2), Some(1), Some(0), Some(42)]);
}

/// **The arguments it was started with.** The body reassigned its parameter
/// before the cut; restarted, it sees what it was called with — not what the
/// parameter held when it was frozen.
#[test]
fn a_restarted_body_receives_its_original_arguments() {
    let aim = "async void Aim(int by) {
                   by = by + 100;
                   int seen = by;
                   await 1.0s;
                   landed = seen;
               }";
    let stopped = stopped(&guard(aim), "Aim", &[Value::Int(7)]);
    let edited = guard(
        "async void Aim(int by) {
             by = by + 100;
             int spot = by;
             await 1.0s;
             landed = spot;
             extra += 1;
         }",
    );

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Restarted);
    assert_eq!(
        (fields.get("landed"), fields.get("extra")),
        (Some(107), Some(1)),
        "7 + 100 — not the 107 the parameter held, plus 100 again"
    );
}

/// A literal a register holds that the new program no longer has cannot be
/// read back: the frame cannot be rebuilt.
#[test]
fn a_literal_missing_from_the_new_program_restarts_the_body() {
    let original = guard(
        r#"async void Call() {
               starts += 1;
               string word = "wind-up";
               await 1.0s;
               landed = 1;
           }"#,
    );
    let edited = guard(
        r#"async void Call() {
               starts += 1;
               string word = "follow";
               await 1.0s;
               landed = 1;
           }"#,
    );
    let stopped = stopped(&original, "Call", &[]);

    let (tier, fields) = resumed_in(&stopped, &edited);

    assert_eq!(tier, ResumeTier::Restarted);
    assert_eq!(
        (fields.get("starts"), fields.get("landed")),
        (Some(2), Some(1))
    );
}

// ─── Tier 5: abandoned ──────────────────────────────────────────────────────

/// The member the body started in is gone: nothing to resume into.
#[test]
fn a_deleted_member_is_abandoned() {
    let stopped = attack_stopped();
    let edited = guard(IDLE);

    let resumed = resume(&stopped.frozen, stopped.fingerprint, &edited);

    assert!(
        matches!(resumed, Err(Abandoned { .. })),
        "{:?}",
        resumed.map(|(_, tier)| tier)
    );
}

/// The member is still there with other parameters: the arguments it was
/// started with no longer fit it.
#[test]
fn a_member_whose_parameters_changed_is_abandoned() {
    let aim = "async void Aim(int by) {
                   int seen = by;
                   await 1.0s;
                   landed = seen;
               }";
    let stopped = stopped(&guard(aim), "Aim", &[Value::Int(7)]);

    for (what, edited) in [
        (
            "a parameter's type",
            "async void Aim(float by) {
                 float seen = by;
                 await 1.0s;
                 landed = 1;
             }",
        ),
        (
            "a parameter added",
            "async void Aim(int by, int how) {
                 int seen = by;
                 await 1.0s;
                 landed = seen;
             }",
        ),
    ] {
        let resumed = resume(&stopped.frozen, stopped.fingerprint, &guard(edited));
        assert!(
            matches!(resumed, Err(Abandoned { .. })),
            "{what}: {:?}",
            resumed.map(|(_, tier)| tier)
        );
    }
}

// ─── A save from before sites ───────────────────────────────────────────────

/// What a save written before frames named their sites reads as: no site, no
/// function fingerprint, no arguments, and a program fingerprint no program
/// now has.
fn from_before_sites(stopped: &Stopped) -> (FrozenMachine, u64) {
    let mut frozen = stopped.frozen.clone();
    for frame in &mut frozen.frames {
        frame.site = String::new();
        frame.fingerprint = 0;
    }
    frozen.arguments = Vec::new();
    (frozen, 14_594_608_129_314_069_874)
}

/// **Tier 4 at best.** Without sites a frame cannot be found again, even in
/// the very program it was frozen in; a member of no parameters restarts, one
/// that took arguments the save did not keep is abandoned.
#[test]
fn a_frozen_machine_from_before_sites_restarts_at_best() {
    let program = guard(&format!("{ATTACK}\n{IDLE}"));
    let (frozen, fingerprint) = from_before_sites(&stopped(&program, "Attack", &[]));
    let resumed = resume(&frozen, fingerprint, &program).map(|(_, tier)| tier);
    assert_eq!(resumed, Ok(ResumeTier::Restarted), "no parameters to miss");

    let aimer = guard(
        "async void Aim(int by) {
             int seen = by;
             await 1.0s;
             landed = seen;
         }",
    );
    let (frozen, fingerprint) = from_before_sites(&stopped(&aimer, "Aim", &[Value::Int(7)]));
    let resumed = resume(&frozen, fingerprint, &aimer).map(|(_, tier)| tier);
    assert!(
        matches!(resumed, Err(Abandoned { .. })),
        "the arguments it took were never saved: {resumed:?}"
    );
}
