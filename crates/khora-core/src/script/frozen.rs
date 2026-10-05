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

//! A script body stopped part-way, written down in the engine's own terms.
//!
//! What a suspended machine *is* belongs to the VM. What a scene stores has to
//! belong to the engine: every scene encoding must be able to read it, a text
//! save must show it, and a change to the VM's internals must not
//! make every save that holds one unreadable. So the VM converts its machine to
//! and from this, and this is what the scene keeps.
//!
//! # Named where the VM numbers
//!
//! A frame names its function rather than indexing it, and a register holding
//! a literal holds the literal's text rather than its slot in the constant
//! table. Both are what a reader of a text save can check against the
//! script; neither moves when the compiler lays the program out differently.
//! Positions *inside* a function — the program counter, a frame's registers —
//! stay numbers: nothing else names them, and the program fingerprint the save
//! carries is what guards them.

use serde::{Deserialize, Serialize};

use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};

/// A machine stopped part-way through a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenMachine {
    /// What finishing it owes.
    pub body: PendingBody,
    /// The whole register file; each frame owns a window of it.
    pub registers: Vec<FrozenValue>,
    /// The call stack, outermost first.
    pub frames: Vec<FrozenFrame>,
    /// The next instruction of the innermost frame's function.
    pub program_counter: u64,
    /// The body's arguments as it was started.
    ///
    /// A parameter is a local the body may reassign, so its frame alone
    /// cannot say what the body was called with. Empty in a save written
    /// before machines recorded them.
    #[serde(default)]
    pub arguments: Vec<FrozenValue>,
}

/// One call on a frozen machine's stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenFrame {
    /// The function it runs, by name.
    pub function: String,
    /// Where its registers begin in the register file.
    pub base: u64,
    /// The instruction its caller resumes at.
    pub return_pc: u64,
    /// The register its result goes to, in the caller's register file.
    pub result: u64,
    /// The named site the frame resumes at. Empty in a save written before
    /// frames recorded it.
    #[serde(default)]
    pub site: String,
    /// The fingerprint of the function it runs. Zero in a save written
    /// before frames recorded it.
    #[serde(default)]
    pub fingerprint: u64,
    /// The locals in scope at its site, and where each sits in its window.
    ///
    /// Written down with the machine because the program that placed them is
    /// gone by the time an edited one reads the save: rebuilding the frame at
    /// the same site in new code moves each local to its new register by name.
    #[serde(default)]
    pub locals: Vec<FrozenLocal>,
    /// The temporaries live across its site, in allocation order, relative to
    /// its window.
    #[serde(default)]
    pub temporaries: Vec<u64>,
}

/// A local in scope where a frozen frame stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenLocal {
    /// Its name.
    pub name: String,
    /// Its type, as written.
    pub ty: String,
    /// The block that declares it — what tells it apart from a local of the
    /// same name it shadows or that shadows it.
    #[serde(default)]
    pub scope: String,
    /// Its register, relative to the frame's window.
    pub register: u64,
}

/// Which body a frozen machine is part-way through, and so what finishing it
/// owes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PendingBody {
    /// An event handler, or a member reached from one: nothing beyond itself.
    Sequence,
    /// `OnSpawn`: finishing it marks the instance spawned.
    Spawn,
    /// `Update`: nothing beyond itself.
    Update,
    /// A schedule's body: finishing it rearms that schedule.
    Timer {
        /// The schedule, by the name of the function its body compiled to —
        /// the `member` of its timer layout.
        #[serde(default)]
        timer: String,
        /// The countdown it rearms to.
        rearm: FrozenValue,
    },
}

/// One register of a frozen machine.
///
/// Not a [`ScriptValue`](super::ScriptValue): a register tells an unset slot
/// from an absent optional, and holds text either as a literal of the program
/// or as something built while running — distinctions a value handed across
/// the engine boundary has no use for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FrozenValue {
    /// An unset register, or `void`.
    Unit,
    /// `int`
    Int(i64),
    /// `float`, and what travels as one — a duration, an angle.
    Float(f32),
    /// `bool`
    Bool(bool),
    /// An entity handle.
    Entity(EntityId),
    /// One of the program's literals, by its text.
    Literal(String),
    /// Text built while running.
    ///
    /// It lived in the frame it was built in and did not outlive that frame,
    /// saved or not; reading it fails the way reading any expired text does.
    Expired,
    /// `Vec2`
    Vec2(Vec2),
    /// `Vec3`
    Vec3(Vec3),
    /// `Vec4`
    Vec4(Vec4),
    /// `Quat`
    Quat(Quaternion),
    /// `Color`
    Color(LinearRgba),
    /// An absent optional.
    Null,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::{PendingSequence, ScriptSnapshot, ScriptValue};

    /// A machine two calls deep, holding one register of every kind.
    fn frozen(body: PendingBody) -> FrozenMachine {
        FrozenMachine {
            body,
            registers: vec![
                FrozenValue::Unit,
                FrozenValue::Int(-42),
                FrozenValue::Float(0.5),
                FrozenValue::Bool(true),
                FrozenValue::Entity(EntityId {
                    index: 7,
                    generation: 3,
                }),
                FrozenValue::Literal("wind-up".to_owned()),
                FrozenValue::Expired,
                FrozenValue::Vec2(Vec2::new(1.0, -2.0)),
                FrozenValue::Vec3(Vec3::new(1.0, 2.0, 3.0)),
                FrozenValue::Vec4(Vec4::new(1.0, 2.0, 3.0, 4.0)),
                FrozenValue::Quat(Quaternion::new(0.0, 0.0, 0.0, 1.0)),
                FrozenValue::Color(LinearRgba::new(0.25, 0.5, 0.75, 1.0)),
                FrozenValue::Null,
            ],
            frames: vec![
                FrozenFrame {
                    function: "Guard::OnSpotted".to_owned(),
                    base: 0,
                    return_pc: 0,
                    result: 0,
                    site: "expr.5d41402a:call.Guard::Attack".to_owned(),
                    fingerprint: 0x0123_4567_89AB_CDEF,
                    locals: Vec::new(),
                    temporaries: Vec::new(),
                },
                FrozenFrame {
                    function: "Guard::Attack".to_owned(),
                    base: 6,
                    return_pc: 3,
                    result: 2,
                    site: "expr.7d793037:await".to_owned(),
                    fingerprint: 0xFEDC_BA98_7654_3210,
                    locals: Vec::new(),
                    temporaries: Vec::new(),
                },
            ],
            program_counter: 4,
            arguments: vec![FrozenValue::Int(7)],
        }
    }

    /// A pending sequence holds its machine directly.
    fn sequence(machine: FrozenMachine) -> PendingSequence {
        PendingSequence {
            fingerprint: 0xDEAD_BEEF_0123_4567,
            remaining: 0.75,
            machine,
        }
    }

    fn every_body() -> Vec<PendingBody> {
        vec![
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
        ]
    }

    /// Writes `value` as JSON and reads it back.
    fn through_json<T>(value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let json = serde_json::to_string(value).expect("serialises");
        serde_json::from_str(&json).expect("parses")
    }

    // ─── Definition saves ───────────────────────────────────────────────────

    /// A text save keeps every register and both frames of a machine, exactly.
    #[test]
    fn a_frozen_sequence_round_trips_through_a_definition_save() {
        let original = sequence(frozen(PendingBody::Sequence));
        assert_eq!(through_json(&original), original);
    }

    /// What finishing the machine owes travels with it, whichever body it is.
    #[test]
    fn every_pending_body_survives_a_definition_save() {
        for body in every_body() {
            let original = frozen(body.clone());
            assert_eq!(through_json(&original), original, "{body:?}");
        }
    }

    /// A machine stopped before it held anything round trips too.
    #[test]
    fn an_empty_machine_round_trips() {
        let original = sequence(FrozenMachine {
            body: PendingBody::Spawn,
            registers: Vec::new(),
            frames: Vec::new(),
            program_counter: 0,
            arguments: Vec::new(),
        });
        assert_eq!(through_json(&original), original);
    }

    /// A whole snapshot carrying a pending sequence comes back as it went in.
    #[test]
    fn a_frozen_snapshot_round_trips_through_a_definition_save() {
        let original = ScriptSnapshot {
            authored: Vec::new(),
            overridden: Vec::new(),
            fields: vec![("fired".to_owned(), ScriptValue::Int(0))],
            state: None,
            state_fields: Vec::new(),
            timers: Vec::new(),
            pending: Some(sequence(frozen(PendingBody::Timer {
                timer: "Guard.__every(0.25)#1".to_owned(),
                rearm: FrozenValue::Float(0.25),
            }))),
            lifecycle: Default::default(),
        };
        assert_eq!(through_json(&original), original);
    }

    /// **What a text save is for.** A machine reads as named frames and
    /// typed registers — something a reader can check against the script —
    /// and not as a list of bytes.
    #[test]
    fn a_frozen_machine_reads_as_named_frames_in_a_definition_save() {
        let original = sequence(frozen(PendingBody::Sequence));
        let json = serde_json::to_string(&original).expect("serialises");

        assert!(
            json.contains(r#""function":"Guard::Attack""#),
            "the frame names its function: {json}"
        );
        assert!(
            json.contains(r#""function":"Guard::OnSpotted""#),
            "every frame does: {json}"
        );
        assert!(
            json.contains(r#"{"Literal":"wind-up"}"#),
            "a literal register holds its text: {json}"
        );
        assert!(
            !json.contains(r#""machine":["#),
            "the machine is not a byte list: {json}"
        );
    }

    /// The machine is written as the plain struct it is — its five fields,
    /// under its own name, with no form wrapped around it.
    #[test]
    fn a_frozen_machine_is_written_as_its_own_fields() {
        let machine = frozen(PendingBody::Update);
        let value = serde_json::to_value(&machine).expect("serialises");
        let object = value.as_object().expect("a machine is an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "arguments",
                "body",
                "frames",
                "program_counter",
                "registers"
            ],
            "{value}"
        );

        let held = serde_json::to_value(sequence(machine)).expect("serialises");
        assert_eq!(
            held["machine"], value,
            "a pending sequence holds the machine as it is"
        );
    }

    /// A machine written as a list of bytes — the form saves held before the
    /// machine had a structure — is not a machine: it is refused, never
    /// guessed at.
    #[test]
    fn a_machine_written_as_bytes_is_refused() {
        let json = r#"{"fingerprint":14594608129314069874,"remaining":1.0,"machine":[4,1,2]}"#;
        let parsed: Result<PendingSequence, _> = serde_json::from_str(json);
        assert!(parsed.is_err(), "a byte list read as {parsed:?}");

        let empty = r#"{"fingerprint":1,"remaining":0.0,"machine":[]}"#;
        let parsed: Result<PendingSequence, _> = serde_json::from_str(empty);
        assert!(parsed.is_err(), "an empty list read as {parsed:?}");
    }
}
