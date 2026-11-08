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

//! What a run leaves in a `#[take]` field goes back to its channel: what the
//! lane did not consume, it did not load. The items return at the front, in
//! their order, ahead of whatever was filed during the run — whether the run
//! recorded or failed. A quote reads without taking, and gives nothing back.

use khora_core::agent::gorna::StrategyId;
use khora_core::event::{Channel, Cursor, WhenFull};
use khora_core::gpu::GpuSetup;
use khora_core::lane::contract::{Lane, LaneReport};
use khora_core::lane::cost::Cost;
use khora_core::lane::erased::{AnyLane, Erased, RunOutcome};
use khora_core::lane::io::{FrameId, InputSource, LaneInputs, OutputSink};
use khora_core::lane::level::Level;
use khora_core::lane::{LaneBus, LaneError, OutputDeck};
use khora_core::Runtime;

use crate::bakery::{BakeFamily, Nothing, OnlyOrders, Order};
use crate::fixtures::level;

const LEVELS: &[Level] = &[level(StrategyId::Balanced, "bake")];

/// What a lane does with the orders it took.
#[derive(Clone, Copy)]
enum Consumes {
    /// Records the first order only, and leaves the rest in its field.
    TheFirst,
    /// `mem::take`s every order.
    Everything,
}

/// A bakery lane that consumes its orders as told, files `during` on the
/// order channel while it records — standing for a request filed while the
/// run is out — and fails when told to.
struct Baker {
    consumes: Consumes,
    channel: Channel<Order>,
    during: Option<Order>,
    fails: bool,
    baked: Vec<Order>,
}

impl Baker {
    fn new(consumes: Consumes, channel: &Channel<Order>) -> Self {
        Self {
            consumes,
            channel: channel.clone(),
            during: None,
            fails: false,
            baked: Vec::new(),
        }
    }
}

impl Lane for Baker {
    type Family = BakeFamily;
    type Inputs<'f> = OnlyOrders;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn cost(&self, _level: usize, _inputs: &OnlyOrders) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut OnlyOrders,
        _out: &mut Nothing,
    ) -> Result<LaneReport, LaneError> {
        match self.consumes {
            Consumes::TheFirst => {
                if !inputs.orders.is_empty() {
                    self.baked.push(inputs.orders.remove(0));
                }
            }
            Consumes::Everything => self.baked.extend(std::mem::take(&mut inputs.orders)),
        }
        if let Some(order) = self.during.take() {
            self.channel.send(order);
        }
        if self.fails {
            return Err(LaneError::InvalidContext {
                expected: "a bake that works",
                received: "a baker told to fail".to_owned(),
            });
        }
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// A runtime whose order channel holds `orders`, and that channel.
fn runtime_with(orders: &[u32]) -> (Runtime, Channel<Order>) {
    let channel = Channel::<Order>::bounded(8, WhenFull::Reject);
    for order in orders {
        channel.send(Order(*order));
    }
    let mut runtime = Runtime::new();
    runtime.resources.insert(channel.clone());
    (runtime, channel)
}

/// What `channel` holds, read without taking.
fn held(channel: &Channel<Order>) -> Vec<Order> {
    channel.read(&mut Cursor::default())
}

/// One run of `lane` over `runtime`, as the scheduler runs it.
fn run(lane: &mut dyn AnyLane, runtime: &Runtime) -> RunOutcome {
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, runtime);
    let mut deck = OutputDeck::new();
    let mut sink = OutputSink::new(&mut deck);
    lane.run(0, &mut source, &mut sink, &mut GpuSetup::headless())
        .outcome
}

/// **What a run leaves in a take field goes back.** Three orders wait; the
/// lane records only the first, and a fourth is filed while it runs. After
/// the run the channel holds the two it left, at the front and in their
/// order, before the one filed during the run.
#[test]
fn what_a_run_leaves_in_a_take_field_goes_back() {
    let (runtime, channel) = runtime_with(&[1, 2, 3]);
    let mut baker = Baker::new(Consumes::TheFirst, &channel);
    baker.during = Some(Order(9));
    let mut lane = Erased::new(baker);

    let outcome = run(&mut lane, &runtime);

    assert!(
        matches!(outcome, RunOutcome::Recorded(_)),
        "the run records: {outcome:?}"
    );
    assert_eq!(
        held(&channel),
        vec![Order(2), Order(3), Order(9)],
        "what the run left goes back in front, in order, before what was filed during it"
    );
}

/// **A run that consumes its take gives nothing back.** The lane
/// `mem::take`s its orders: the channel is left empty, as today's lanes
/// leave it.
#[test]
fn a_run_that_consumes_its_take_gives_nothing_back() {
    let (runtime, channel) = runtime_with(&[1, 2, 3]);
    let mut lane = Erased::new(Baker::new(Consumes::Everything, &channel));

    let outcome = run(&mut lane, &runtime);

    assert!(
        matches!(outcome, RunOutcome::Recorded(_)),
        "the run records: {outcome:?}"
    );
    assert!(
        channel.is_empty(),
        "nothing left, nothing given back: {:?}",
        held(&channel)
    );
}

/// **A failed run gives back what it left.** Two orders wait; the lane
/// records the first, leaves the second and fails: the second is back on
/// the channel.
#[test]
fn a_failed_run_gives_back_what_it_left() {
    let (runtime, channel) = runtime_with(&[1, 2]);
    let mut baker = Baker::new(Consumes::TheFirst, &channel);
    baker.fails = true;
    let mut lane = Erased::new(baker);

    let outcome = run(&mut lane, &runtime);

    assert!(
        matches!(outcome, RunOutcome::Failed(_)),
        "the run fails: {outcome:?}"
    );
    assert_eq!(
        held(&channel),
        vec![Order(2)],
        "the order the failed run left goes back"
    );
}

/// **A quote neither takes nor gives back.** Quoting reads the orders to
/// price them and leaves the channel exactly as it was — nothing taken, and
/// nothing given back twice.
#[test]
fn a_quote_neither_takes_nor_gives_back() {
    let (runtime, channel) = runtime_with(&[1, 2, 3]);
    let lane = Erased::new(Baker::new(Consumes::TheFirst, &channel));
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, &runtime);

    let quoted = lane.quote(&mut source);

    assert!(quoted.is_some(), "the lane is quoted");
    assert_eq!(
        held(&channel),
        vec![Order(1), Order(2), Order(3)],
        "the quote left the channel as it was"
    );
}

/// **Derived inputs give back what their take fields hold.** The orders are
/// fetched — taken — and the first removed from the field: giving the
/// inputs back puts the other two at the front of the channel, in order.
#[test]
fn derived_inputs_give_back_what_their_take_fields_hold() {
    let (runtime, channel) = runtime_with(&[1, 2, 3]);
    let bus = LaneBus::new();
    let mut source = InputSource::new(FrameId(0), &bus, &runtime);
    let mut inputs = OnlyOrders::fetch(&mut source).expect("the channel is held");
    assert!(channel.is_empty(), "the premise: the fetch took the orders");
    inputs.orders.remove(0);

    inputs.give_back(&mut source);

    assert_eq!(held(&channel), vec![Order(2), Order(3)]);
}

/// A bakery lane that cannot be prepared: its oven never lights.
struct ColdOven;

impl Lane for ColdOven {
    type Family = BakeFamily;
    type Inputs<'f> = OnlyOrders;

    fn levels(&self) -> &[Level] {
        LEVELS
    }

    fn prepare(
        &mut self,
        _level: usize,
        _gpu: &mut GpuSetup<'_>,
        _key: Option<khora_core::lane::io::TargetKey>,
    ) -> Result<(), LaneError> {
        Err(LaneError::InvalidContext {
            expected: "an oven that lights",
            received: "a cold oven".to_owned(),
        })
    }

    fn cost(&self, _level: usize, _inputs: &OnlyOrders) -> Cost {
        Cost::default()
    }

    fn record(
        &mut self,
        _level: usize,
        inputs: &mut OnlyOrders,
        _out: &mut Nothing,
    ) -> Result<LaneReport, LaneError> {
        inputs.orders.clear();
        Ok(LaneReport {
            done: 1.0,
            stalled: false,
            message: None,
        })
    }
}

/// **A run that cannot be prepared gives back everything it took.** Two
/// orders wait; the lane fails to prepare, so it never records: both orders
/// are back on the channel, in their order.
#[test]
fn a_run_that_cannot_be_prepared_gives_back_everything_it_took() {
    let (runtime, channel) = runtime_with(&[1, 2]);
    let mut lane = Erased::new(ColdOven);

    let outcome = run(&mut lane, &runtime);

    assert!(
        matches!(outcome, RunOutcome::Failed(_)),
        "the run fails: {outcome:?}"
    );
    assert_eq!(
        held(&channel),
        vec![Order(1), Order(2)],
        "a run that never recorded loaded nothing"
    );
}
