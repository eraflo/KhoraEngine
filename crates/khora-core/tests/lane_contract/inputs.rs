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

//! What a derived `LaneInputs` declares, and what its `fetch` reaches.

use std::any::TypeId;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_core::lane::guard::SourceKind;
use khora_core::lane::io::{Budget, Declaration, Declared, FrameId, InputSource, LaneInputs};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::Runtime;

use crate::fixtures::{Contacts, Counter, EveryInput, MixSettings, Positions, SharedCounter};

/// The comparable part of a declared input: field, source, type.
fn entries(declared: &[Declared<SourceKind>]) -> BTreeSet<(&'static str, SourceKind, TypeId)> {
    declared
        .iter()
        .map(|entry| (entry.field, entry.kind, entry.ty))
        .collect()
}

/// **The contract a scheduler relies on.** What a lane declares is what its
/// fetch reaches, field for field: contention is generated from the
/// declaration, so a source reached but not declared would be a race the
/// scheduler cannot see, and a source declared but not reached would
/// serialise a wave for nothing.
#[test]
fn inputs_declare_exactly_what_they_fetch() {
    let frame = FrameId(7);
    let mut bus = LaneBus::new();
    bus.set_frame(frame);
    bus.publish(Positions(vec![1.0, 2.0]));
    let mut deck = OutputDeck::new();
    *deck.slot::<Contacts>() = Contacts(3);
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 0.5 });
    let counter: SharedCounter = Arc::new(Mutex::new(Counter(0)));
    runtime.resources.insert(counter);
    let budget = Budget {
        time: Duration::from_millis(2),
    };

    let mut declared = Declaration::default();
    EveryInput::declare(&mut declared);

    let mut source = InputSource::new(frame, &bus, &runtime)
        .with_deck(&deck)
        .with_budget(budget);
    let inputs = EveryInput::fetch(&mut source).expect("every input is present and fresh");

    assert_eq!(inputs.view, &Positions(vec![1.0, 2.0]));
    assert_eq!(inputs.earlier, Some(&Contacts(3)));
    assert_eq!(inputs.settings, &MixSettings { gain: 0.5 });
    assert_eq!(
        inputs.counter.0, 0,
        "the lock reaches the registered counter"
    );
    assert_eq!(inputs.budget, budget);
    drop(inputs);

    let expected: BTreeSet<_> = [
        ("view", SourceKind::Bus, TypeId::of::<Positions>()),
        ("earlier", SourceKind::Deck, TypeId::of::<Contacts>()),
        (
            "settings",
            SourceKind::Resource,
            TypeId::of::<MixSettings>(),
        ),
        ("counter", SourceKind::Locked, TypeId::of::<SharedCounter>()),
        ("budget", SourceKind::Budget, TypeId::of::<Budget>()),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        entries(declared.inputs()),
        expected,
        "one declared input per field, by the field's source and the type looked up"
    );
    assert_eq!(
        entries(source.touched().inputs()),
        entries(declared.inputs()),
        "the fetch reached exactly what the derive declared"
    );

    let contention = declared.contention();
    assert_eq!(contention.reads, vec![TypeId::of::<MixSettings>()]);
    assert_eq!(contention.writes, vec![TypeId::of::<SharedCounter>()]);
    assert!(
        contention.deck.is_empty(),
        "reading an earlier phase's slot writes no slot"
    );
}

// ─── Field names ────────────────────────────────────────────────────────────

use khora_core::lane::guard::{InputRefusal, Reason};
use khora_macros::LaneInputs;

/// A field named with a raw identifier.
#[derive(LaneInputs)]
#[allow(dead_code)]
pub struct RawNamed<'f> {
    #[bus]
    pub r#type: &'f Positions,
}

/// A refusal and a declaration name the field as the struct names it to a
/// reader — `type`, not the `r#type` spelling Rust needs to accept it.
#[test]
fn a_raw_identifier_field_is_named_without_its_prefix() {
    let mut declared = Declaration::default();
    RawNamed::declare(&mut declared);
    assert_eq!(declared.inputs()[0].field, "type");

    let bus = LaneBus::new();
    let runtime = Runtime::new();
    let mut source = InputSource::new(FrameId(1), &bus, &runtime);
    assert_eq!(
        RawNamed::fetch(&mut source).err(),
        Some(InputRefusal {
            field: "type",
            source: SourceKind::Bus,
            reason: Reason::Missing,
        })
    );
}
