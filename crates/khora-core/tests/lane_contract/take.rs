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

//! What a `#[take]` field declares and fetches: the requests the runtime's
//! `Channel<T>` holds, drained, declared as a lock of that channel.

use std::any::TypeId;
use std::collections::BTreeSet;

use khora_core::event::{Channel, WhenFull};
use khora_core::lane::guard::{InputRefusal, Reason, SourceKind};
use khora_core::lane::io::{Declaration, FrameId, InputSource, LaneInputs};
use khora_core::lane::LaneBus;
use khora_core::Runtime;

use crate::bakery::{OnlyOrders, Order, TakesOrders};
use crate::fixtures::MixSettings;

/// A runtime whose order channel holds `orders`.
fn runtime_with(orders: &[u32]) -> Runtime {
    let channel = Channel::<Order>::bounded(8, WhenFull::Reject);
    for order in orders {
        channel.send(Order(*order));
    }
    let mut runtime = Runtime::new();
    runtime.resources.insert(channel);
    runtime.resources.insert(MixSettings { gain: 1.0 });
    runtime
}

fn channel(runtime: &Runtime) -> &Channel<Order> {
    runtime
        .resources
        .get::<Channel<Order>>()
        .expect("the order channel")
}

/// **A take declares the channel it drains, as a lock.** Draining empties
/// the channel for every other reader, so two lanes taking it cannot run side
/// by side: the channel is in the lane's contention among what it locks, not
/// what it reads.
#[test]
fn a_take_declares_the_channel_it_drains_as_a_lock() {
    let mut declared = Declaration::default();
    TakesOrders::declare(&mut declared);

    let inputs: BTreeSet<_> = declared
        .inputs()
        .iter()
        .map(|entry| (entry.field, entry.kind, entry.ty))
        .collect();
    assert!(
        inputs.contains(&("orders", SourceKind::Take, TypeId::of::<Channel<Order>>())),
        "the take is declared as the channel it drains: {inputs:?}"
    );

    let contention = declared.contention();
    assert_eq!(contention.writes, vec![TypeId::of::<Channel<Order>>()]);
    assert_eq!(contention.reads, vec![TypeId::of::<MixSettings>()]);
}

/// **A take drains what the channel holds, and touches what it declares.**
/// The orders come out in the order they were sent; the channel is left
/// empty; a second fetch takes only what arrived since.
#[test]
fn a_take_drains_what_the_channel_holds() {
    let runtime = runtime_with(&[1, 2]);
    let bus = LaneBus::new();

    let mut source = InputSource::new(FrameId(0), &bus, &runtime);
    let inputs = OnlyOrders::fetch(&mut source).expect("the channel is held");
    assert_eq!(inputs.orders, vec![Order(1), Order(2)]);
    assert!(channel(&runtime).is_empty(), "taken, not read");

    let mut declared = Declaration::default();
    OnlyOrders::declare(&mut declared);
    assert_eq!(
        source.touched(),
        declared,
        "the fetch reached what the take declares"
    );

    channel(&runtime).send(Order(3));
    let mut source = InputSource::new(FrameId(1), &bus, &runtime);
    let again = OnlyOrders::fetch(&mut source).expect("the channel is held");
    assert_eq!(again.orders, vec![Order(3)], "only what arrived since");
}

/// **A take of a channel the runtime does not hold is refused as missing**,
/// naming the field and the source.
#[test]
fn a_take_of_a_channel_the_runtime_does_not_hold_is_refused() {
    let runtime = Runtime::new();
    let bus = LaneBus::new();

    let mut source = InputSource::new(FrameId(0), &bus, &runtime);
    let refused = OnlyOrders::fetch(&mut source).map(|inputs| inputs.orders);

    assert_eq!(
        refused,
        Err(InputRefusal {
            field: "orders",
            source: SourceKind::Take,
            reason: Reason::Missing,
        })
    );
}

/// **A fetch refused after its take leaves the requests where they were.**
/// The bakery's lane takes its orders, then reads a runtime entry the runtime
/// does not hold: the fetch is refused, the lane never sees the orders — so
/// they must still wait in the channel for the next run. Whether they survive
/// cannot depend on the order the lane's fields are written in.
#[test]
fn a_fetch_refused_after_its_take_leaves_the_requests_in_the_channel() {
    let orders = Channel::<Order>::bounded(8, WhenFull::Reject);
    orders.send(Order(1));
    orders.send(Order(2));
    let mut runtime = Runtime::new();
    runtime.resources.insert(orders);
    // No `MixSettings`: the field after the take is refused.
    let bus = LaneBus::new();

    let mut source = InputSource::new(FrameId(0), &bus, &runtime);
    let refused = TakesOrders::fetch(&mut source).map(|inputs| inputs.orders);

    assert_eq!(
        refused.map(|_| ()),
        Err(InputRefusal {
            field: "settings",
            source: SourceKind::Resource,
            reason: Reason::Missing,
        })
    );
    assert_eq!(
        channel(&runtime).read(&mut khora_core::event::Cursor::default()),
        vec![Order(1), Order(2)],
        "a refused fetch took the orders its lane never saw"
    );
}
