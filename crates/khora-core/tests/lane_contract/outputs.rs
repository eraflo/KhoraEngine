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

//! What a derived `LaneOutputs` commits, and the guards its commit runs.

use khora_core::lane::guard::{OutputRefusal, Reason, SinkKind};
use khora_core::lane::io::{LaneOutputs, OutputSink};
use khora_core::lane::OutputDeck;
use khora_macros::LaneOutputs;

use crate::fixtures::Echoes;

/// Two fields writing one deck slot.
#[derive(LaneOutputs)]
pub struct TwiceEchoes {
    #[deck]
    pub first: Echoes,
    #[deck]
    pub second: Echoes,
}

/// A deck slot holds one value per frame. Two output fields of one slot type
/// would each commit into it and the first value would be lost without a
/// word; the commit guard refuses the second write instead
/// (`Reason::WrittenTwice`).
#[test]
fn a_deck_slot_written_twice_is_refused() {
    let mut deck = OutputDeck::new();
    let committed = {
        let mut sink = OutputSink::new(&mut deck);
        let mut outputs = TwiceEchoes::open(&mut sink).expect("deck outputs open");
        outputs.first = Echoes(1);
        outputs.second = Echoes(2);
        outputs.commit(&mut sink)
    };

    assert_eq!(
        committed,
        Err(OutputRefusal {
            field: "second",
            sink: SinkKind::Deck,
            reason: Reason::WrittenTwice,
        }),
        "the slot now holds {:?}: `first` was overwritten",
        deck.get::<Echoes>()
    );
}

// ─── Slots of different types ───────────────────────────────────────────────

use crate::fixtures::{Contacts, MixedFrames};

/// Two fields writing two different slots.
#[derive(LaneOutputs)]
pub struct MixedAndContacts {
    #[deck]
    pub mixed: MixedFrames,
    #[deck]
    pub contacts: Contacts,
}

/// The written-twice guard is per slot type: a family writing two different
/// slots (physics writes its step result and its contacts) commits both.
#[test]
fn two_deck_slots_of_different_types_both_commit() {
    let mut deck = OutputDeck::new();
    let committed = {
        let mut sink = OutputSink::new(&mut deck);
        let mut outputs = MixedAndContacts::open(&mut sink).expect("deck outputs open");
        outputs.mixed = MixedFrames(4);
        outputs.contacts = Contacts(9);
        outputs.commit(&mut sink)
    };

    assert_eq!(committed, Ok(()));
    assert_eq!(deck.get::<MixedFrames>(), Some(&MixedFrames(4)));
    assert_eq!(deck.get::<Contacts>(), Some(&Contacts(9)));
}
