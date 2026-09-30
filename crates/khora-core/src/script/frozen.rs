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
//! belong to the engine: every serialisation strategy must be able to read it,
//! a Definition save must show it, and a change to the VM's internals must not
//! make every save that holds one unreadable. So the VM converts its machine to
//! and from this, and this is what the scene keeps.
//!
//! # Named where the VM numbers
//!
//! A frame names its function rather than indexing it, and a register holding
//! a literal holds the literal's text rather than its slot in the constant
//! table. Both are what a reader of a Definition save can check against the
//! script; neither moves when the compiler lays the program out differently.
//! Positions *inside* a function — the program counter, a frame's registers —
//! stay numbers: nothing else names them, and the program fingerprint the save
//! carries is what guards them.

use bincode::config::{Configuration, Limit, LittleEndian, Varint};
use bincode::de::read::Reader;
use bincode::de::DecoderImpl;
use bincode::de::{BorrowDecoder, Decoder};
use bincode::enc::write::SizeWriter;
use bincode::enc::{Encoder, EncoderImpl};
use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use serde::{Deserialize, Serialize};

use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};

/// A suspended machine, as a save holds it.
///
/// Two forms, because saves were written before the structured one existed.
/// Only [`Frozen`](Self::Frozen) is ever written; [`Legacy`](Self::Legacy) is
/// read so those saves still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SuspendedMachine {
    /// The VM's own encoding of an event handler's machine, as saves held it
    /// before the machine had a structured form. Opaque to everything but the
    /// VM, and an event handler's sequence — the only body that could be saved
    /// then.
    ///
    /// First, so a self-describing format tries it first: a byte list is a
    /// list, and no structured machine is one.
    Legacy(Vec<u8>),
    /// A machine in the engine's terms.
    Frozen(FrozenMachine),
}

/// The marker a binary save writes where a legacy machine wrote its length.
///
/// bincode is positional and a legacy machine starts with its byte count, so
/// the structured form needs a first value no byte count can be.
const FROZEN_MARKER: u64 = u64::MAX;

/// How a structured machine is written inside a binary save.
///
/// Its own codec rather than the save's, for the limit: every list and every
/// text in it carries a count read from the save, and bincode sizes an
/// allocation from a count before reading what it counts. Unbounded, a damaged
/// count of 2^60 registers panics or aborts the process — the whole scene load
/// for one bad byte. Bounded, it is an error.
///
/// The bound holds on the writing side too — [`FrozenMachine::fits_a_save`] —
/// because bincode checks a limit only when decoding: a machine written past
/// it would be a save the engine cannot read back.
const MACHINE_CODEC: Configuration<LittleEndian, Varint, Limit<MACHINE_LIMIT>> =
    bincode::config::standard().with_limit::<MACHINE_LIMIT>();

/// The most a structured machine may take to read back, in bytes.
///
/// Generous on purpose: a literal register is written as its whole text, so a
/// long literal carried down a deep call stack is written once per frame. What
/// the bound rules out is a count nobody wrote.
const MACHINE_LIMIT: usize = 64 << 20;

/// How much of a legacy machine is read at a time.
///
/// Its length comes from the save, so it is not trusted to size an allocation
/// up front: a damaged length would ask for gigabytes before the read that
/// fails on the missing bytes.
const LEGACY_CHUNK: usize = 4096;

impl Encode for SuspendedMachine {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        match self {
            Self::Legacy(bytes) => bytes.encode(encoder),
            Self::Frozen(machine) => {
                FROZEN_MARKER.encode(encoder)?;
                machine.encode(&mut EncoderImpl::new(encoder.writer(), MACHINE_CODEC))
            }
        }
    }
}

impl<Context> Decode<Context> for SuspendedMachine {
    fn decode<D: Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        decode_suspended(decoder)
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for SuspendedMachine {
    fn borrow_decode<D: BorrowDecoder<'de, Context = Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        decode_suspended(decoder)
    }
}

/// Reads either form: the marker, then a structured machine; or a byte count,
/// then that many bytes of a legacy one.
fn decode_suspended<D: Decoder>(decoder: &mut D) -> Result<SuspendedMachine, DecodeError> {
    let head = u64::decode(decoder)?;
    if head == FROZEN_MARKER {
        let mut bounded = DecoderImpl::new(decoder.reader(), MACHINE_CODEC, ());
        return FrozenMachine::decode(&mut bounded).map(SuspendedMachine::Frozen);
    }

    let len = usize::try_from(head).map_err(|_| DecodeError::OutsideUsizeRange(head))?;
    decoder.claim_container_read::<u8>(len)?;
    let mut bytes = Vec::with_capacity(len.min(LEGACY_CHUNK));
    let mut chunk = [0u8; LEGACY_CHUNK];
    while bytes.len() < len {
        let take = (len - bytes.len()).min(LEGACY_CHUNK);
        decoder.reader().read(&mut chunk[..take])?;
        bytes.extend_from_slice(&chunk[..take]);
    }
    Ok(SuspendedMachine::Legacy(bytes))
}

/// A machine stopped part-way through a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct FrozenMachine {
    /// What finishing it owes.
    pub body: PendingBody,
    /// The whole register file; each frame owns a window of it.
    pub registers: Vec<FrozenValue>,
    /// The call stack, outermost first.
    pub frames: Vec<FrozenFrame>,
    /// The next instruction of the innermost frame's function.
    pub program_counter: u64,
}

impl FrozenMachine {
    /// Whether a binary save holding this machine reads back.
    ///
    /// Counts what decoding it will claim against the reading limit: the bytes
    /// it is written as, plus the lists it holds at their size in memory,
    /// which the decoder claims whole before it reads their first element.
    pub fn fits_a_save(&self) -> bool {
        let mut size = SizeWriter::default();
        if bincode::encode_into_writer(self, &mut size, MACHINE_CODEC).is_err() {
            return false;
        }
        let lists = self.registers.len() * std::mem::size_of::<FrozenValue>()
            + self.frames.len() * std::mem::size_of::<FrozenFrame>();
        size.bytes_written.saturating_add(lists) <= MACHINE_LIMIT
    }
}

/// One call on a frozen machine's stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct FrozenFrame {
    /// The function it runs, by name.
    pub function: String,
    /// Where its registers begin in the register file.
    pub base: u64,
    /// The instruction its caller resumes at.
    pub return_pc: u64,
    /// The register its result goes to, in the caller's register file.
    pub result: u64,
}

/// Which body a frozen machine is part-way through, and so what finishing it
/// owes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub enum PendingBody {
    /// An event handler, or a member reached from one: nothing beyond itself.
    Sequence,
    /// `OnSpawn`: finishing it marks the instance spawned.
    Spawn,
    /// `Update`: nothing beyond itself.
    Update,
    /// A schedule's body: finishing it rearms that schedule.
    Timer {
        /// The schedule, by its position in the behavior. A position is safe
        /// here only because the save's program fingerprint changes with the
        /// behavior's members, and a sequence whose fingerprint no longer
        /// matches is never resumed.
        index: u32,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
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

    fn config() -> bincode::config::Configuration {
        bincode::config::standard()
    }

    /// Encodes `value` and decodes it back, asserting the decoder used every
    /// byte the encoder wrote.
    fn through_bincode<T>(value: &T) -> T
    where
        T: Encode + Decode<()>,
    {
        let bytes = bincode::encode_to_vec(value, config()).expect("encodes");
        let (decoded, read): (T, usize) =
            bincode::decode_from_slice(&bytes, config()).expect("decodes");
        assert_eq!(
            read,
            bytes.len(),
            "the decoder read exactly what was written"
        );
        decoded
    }

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
                },
                FrozenFrame {
                    function: "Guard::Attack".to_owned(),
                    base: 6,
                    return_pc: 3,
                    result: 2,
                },
            ],
            program_counter: 4,
        }
    }

    fn sequence(machine: SuspendedMachine) -> PendingSequence {
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
                index: 2,
                rearm: FrozenValue::Float(0.5),
            },
            PendingBody::Timer {
                index: 0,
                rearm: FrozenValue::Null,
            },
        ]
    }

    /// The shape a binary save held before the machine had a structured form.
    #[derive(bincode::Encode)]
    struct OldPendingSequence {
        fingerprint: u64,
        remaining: f32,
        machine: Vec<u8>,
    }

    // ─── Binary saves ───────────────────────────────────────────────────────

    /// A binary save keeps every register and both frames of a structured
    /// machine, exactly.
    #[test]
    fn a_frozen_sequence_round_trips_through_a_binary_save() {
        let original = sequence(SuspendedMachine::Frozen(frozen(PendingBody::Sequence)));
        assert_eq!(through_bincode(&original), original);
    }

    /// What finishing the machine owes travels with it, whichever body it is.
    #[test]
    fn every_pending_body_survives_a_binary_save() {
        for body in every_body() {
            let original = SuspendedMachine::Frozen(frozen(body.clone()));
            assert_eq!(through_bincode(&original), original, "{body:?}");
        }
    }

    /// The borrowing decoder reads what the owning one reads.
    #[test]
    fn the_borrowing_decoder_reads_both_forms() {
        for original in [
            SuspendedMachine::Frozen(frozen(PendingBody::Update)),
            SuspendedMachine::Legacy(vec![4, 1, 2, 0, 9]),
            SuspendedMachine::Legacy(Vec::new()),
        ] {
            let bytes = bincode::encode_to_vec(&original, config()).expect("encodes");
            let (decoded, read): (SuspendedMachine, usize) =
                bincode::borrow_decode_from_slice(&bytes, config()).expect("decodes");
            assert_eq!(decoded, original);
            assert_eq!(read, bytes.len());
        }
    }

    #[test]
    fn a_legacy_machine_round_trips_through_a_binary_save() {
        let original = sequence(SuspendedMachine::Legacy(vec![
            4, 1, 2, 0, 0, 0, 2, 2, 0, 0, 0, 1, 1, 1, 2, 2, 0,
        ]));
        assert_eq!(through_bincode(&original), original);
    }

    #[test]
    fn an_empty_legacy_machine_round_trips() {
        let original = sequence(SuspendedMachine::Legacy(Vec::new()));
        assert_eq!(through_bincode(&original), original);

        let parsed: SuspendedMachine = serde_json::from_str("[]").expect("an empty list parses");
        assert_eq!(parsed, SuspendedMachine::Legacy(Vec::new()));
        assert_eq!(through_bincode(&parsed), parsed);
    }

    /// **The compatibility promise.** A save written when the machine was a
    /// byte list reads as that byte list — at every width the byte count can
    /// have been written with (one byte, then two, then four).
    #[test]
    fn a_binary_save_from_before_the_structured_machine_reads_as_legacy() {
        for length in [0usize, 3, 250, 251, 300, 70_000] {
            let bytes: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
            let old = OldPendingSequence {
                fingerprint: 14_594_608_129_314_069_874,
                remaining: 1.0,
                machine: bytes.clone(),
            };
            let encoded = bincode::encode_to_vec(&old, config()).expect("the old shape encodes");

            let (decoded, read): (PendingSequence, usize) =
                bincode::decode_from_slice(&encoded, config()).expect("an old save still decodes");

            assert_eq!(read, encoded.len(), "{length} bytes: read to the end");
            assert_eq!(
                decoded,
                PendingSequence {
                    fingerprint: 14_594_608_129_314_069_874,
                    remaining: 1.0,
                    machine: SuspendedMachine::Legacy(bytes),
                },
                "{length} bytes"
            );
        }
    }

    /// Writing a legacy machine back writes exactly what the old shape wrote,
    /// so an older build can still read a save this one re-saved untouched.
    #[test]
    fn a_legacy_machine_encodes_as_the_byte_list_it_holds() {
        for bytes in [vec![], vec![4, 1, 2], vec![7; 300]] {
            assert_eq!(
                bincode::encode_to_vec(SuspendedMachine::Legacy(bytes.clone()), config())
                    .expect("encodes"),
                bincode::encode_to_vec(&bytes, config()).expect("encodes"),
            );
        }
    }

    /// The structured form opens with a value no byte count can be, which is
    /// what lets one decoder tell the two forms apart.
    #[test]
    fn a_frozen_machine_opens_with_a_marker_no_byte_count_can_be() {
        let marker = bincode::encode_to_vec(u64::MAX, config()).expect("encodes");
        let machine = frozen(PendingBody::Spawn);
        let encoded = bincode::encode_to_vec(SuspendedMachine::Frozen(machine.clone()), config())
            .expect("encodes");

        assert!(
            encoded.starts_with(&marker),
            "starts with the marker: {encoded:?}"
        );
        assert_eq!(
            &encoded[marker.len()..],
            bincode::encode_to_vec(&machine, config())
                .expect("encodes")
                .as_slice(),
            "and the machine follows it, nothing else"
        );
    }

    /// Positional encodings put values end to end: a decoder that reads one
    /// byte too many or too few corrupts everything after it.
    #[test]
    fn decoding_takes_exactly_its_own_bytes() {
        let list = vec![
            sequence(SuspendedMachine::Legacy(vec![1, 2, 3])),
            sequence(SuspendedMachine::Frozen(frozen(PendingBody::Update))),
            sequence(SuspendedMachine::Legacy(Vec::new())),
            sequence(SuspendedMachine::Frozen(frozen(PendingBody::Timer {
                index: 1,
                rearm: FrozenValue::Float(2.0),
            }))),
        ];
        assert_eq!(through_bincode(&list), list);

        let followed = (
            SuspendedMachine::Frozen(frozen(PendingBody::Sequence)),
            0xABCD_u32,
            SuspendedMachine::Legacy(vec![9, 9]),
            String::from("after"),
        );
        assert_eq!(through_bincode(&followed), followed);
    }

    /// A save cut short is refused, never read past its end and never a panic.
    #[test]
    fn a_truncated_binary_machine_is_refused() {
        for original in [
            SuspendedMachine::Frozen(frozen(PendingBody::Update)),
            SuspendedMachine::Legacy(vec![5; 40]),
        ] {
            let bytes = bincode::encode_to_vec(&original, config()).expect("encodes");
            for cut in 0..bytes.len() {
                let decoded: Result<(SuspendedMachine, usize), _> =
                    bincode::decode_from_slice(&bytes[..cut], config());
                assert!(
                    decoded.is_err(),
                    "{cut} of {} bytes decoded as {decoded:?}",
                    bytes.len()
                );
            }
        }
    }

    // ─── Definition saves ───────────────────────────────────────────────────

    /// A scene moves between strategies: a machine read from a Definition save
    /// can be written into a binary one and back without losing anything.
    #[test]
    fn a_frozen_snapshot_round_trips_through_a_definition_save_and_a_binary_one() {
        let original = ScriptSnapshot {
            fields: vec![("fired".to_owned(), ScriptValue::Int(0))],
            state: None,
            state_fields: Vec::new(),
            timers: Vec::new(),
            pending: Some(sequence(SuspendedMachine::Frozen(frozen(
                PendingBody::Timer {
                    index: 3,
                    rearm: FrozenValue::Float(0.25),
                },
            )))),
        };

        let json = serde_json::to_string(&original).expect("serialises");
        let parsed: ScriptSnapshot = serde_json::from_str(&json).expect("parses");
        assert_eq!(parsed, original, "through JSON");
        assert_eq!(through_bincode(&parsed), original, "then through bincode");
    }

    /// **What a Definition save is for.** A machine reads as named frames and
    /// typed registers — something a reader can check against the script —
    /// and not as a list of bytes, even after it went through a binary save.
    #[test]
    fn a_frozen_machine_reads_as_named_frames_in_a_definition_save() {
        let original = sequence(SuspendedMachine::Frozen(frozen(PendingBody::Sequence)));
        let json = serde_json::to_string(&through_bincode(&original)).expect("serialises");

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

    /// A Definition save from before the structured machine reads as the byte
    /// list it held — and writing it into a binary save gives exactly what the
    /// old binary save would have held.
    #[test]
    fn a_definition_save_from_before_the_structured_machine_reads_as_legacy() {
        let json = r#"{"fingerprint":14594608129314069874,"remaining":1.0,"machine":[4,1,2]}"#;
        let parsed: PendingSequence = serde_json::from_str(json).expect("an old save parses");
        assert_eq!(parsed.machine, SuspendedMachine::Legacy(vec![4, 1, 2]));

        let old = OldPendingSequence {
            fingerprint: 14_594_608_129_314_069_874,
            remaining: 1.0,
            machine: vec![4, 1, 2],
        };
        assert_eq!(
            bincode::encode_to_vec(&parsed, config()).expect("encodes"),
            bincode::encode_to_vec(&old, config()).expect("encodes"),
        );
    }

    /// A damaged count in a structured machine is refused like a damaged
    /// legacy length: a save that claims 2^60 registers and holds none is an
    /// error, never an allocation sized from the claim — which panics
    /// ("capacity overflow") or aborts the process, taking the scene load
    /// with it.
    #[test]
    fn a_structured_machine_claiming_more_registers_than_it_holds_is_refused() {
        // The marker, the body (`Sequence`), then a register count and nothing.
        let bytes =
            bincode::encode_to_vec((u64::MAX, 0u32, 1u64 << 60), config()).expect("encodes");

        let decoded = std::panic::catch_unwind(|| {
            bincode::decode_from_slice::<SuspendedMachine, _>(&bytes, config()).map(|(m, _)| m)
        });

        match decoded {
            Ok(result) => assert!(result.is_err(), "decoded as {result:?}"),
            Err(_) => panic!("decoding a damaged register count panicked instead of refusing"),
        }
    }
}
