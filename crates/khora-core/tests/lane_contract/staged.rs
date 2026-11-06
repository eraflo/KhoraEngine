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

//! What a `#[staged]` output declares and where it goes: into the run's
//! staging, never the frame's deck, and from there into the reserve.

use std::any::TypeId;

use khora_core::lane::guard::{OutputRefusal, Reason, SinkKind};
use khora_core::lane::io::{Declaration, LaneOutputs, OutputSink};
use khora_core::lane::{OutputDeck, Reserve, Staging};

use crate::bakery::{batches, Abandoned, BakeOutputs, Loaf};

/// **A staged output declares what it stages**: the field, the `Staged`
/// sink, and the output type the reserve keeps it under.
#[test]
fn a_staged_output_declares_what_it_stages() {
    let mut declared = Declaration::default();
    BakeOutputs::declare(&mut declared);

    let outputs: Vec<_> = declared
        .outputs()
        .iter()
        .map(|entry| (entry.field, entry.kind, entry.ty))
        .collect();
    assert_eq!(
        outputs,
        vec![("loaves", SinkKind::Staged, TypeId::of::<Loaf>())]
    );
}

/// **A staged output goes into the run's staging, never the frame's deck.**
/// It opens empty — a run stages what it made, not what an earlier one did —
/// and its commit leaves the deck untouched; the staging, received by the
/// reserve, gives the loaves back in the order the lane wrote them.
#[test]
fn a_staged_output_goes_into_the_runs_staging_not_the_deck() {
    let log = Abandoned::default();
    let mut deck = OutputDeck::new();
    let mut staging = Staging::new();
    {
        let mut sink = OutputSink::new(&mut deck).with_staging(&mut staging);
        let mut outputs = BakeOutputs::open(&mut sink).expect("a staged output opens");
        assert!(outputs.loaves.is_empty(), "nothing staged yet");
        outputs.loaves.push(Loaf::new(1, 1, &log));
        outputs.loaves.push(Loaf::new(2, 1, &log));
        outputs.commit(&mut sink).expect("the loaves are staged");
    }

    assert!(deck.get::<Vec<Loaf>>().is_none(), "no deck slot holds them");
    assert!(deck.get::<Loaf>().is_none(), "no deck slot holds them");
    assert!(!staging.is_empty());

    let reserve = Reserve::new();
    reserve.receive(staging);
    assert_eq!(batches(&reserve.take::<Loaf>()), vec![(1, 1), (2, 1)]);
}

/// **A staged output on a sink lent no staging is refused**, naming the
/// field: what the lane made would be lost.
#[test]
fn a_staged_output_without_a_staging_is_refused() {
    let log = Abandoned::default();
    let mut deck = OutputDeck::new();
    let mut sink = OutputSink::new(&mut deck);
    let mut outputs = BakeOutputs::open(&mut sink).expect("a staged output opens");
    outputs.loaves.push(Loaf::new(1, 1, &log));

    assert_eq!(
        outputs.commit(&mut sink),
        Err(OutputRefusal {
            field: "loaves",
            sink: SinkKind::Staged,
            reason: Reason::Missing,
        })
    );
}
