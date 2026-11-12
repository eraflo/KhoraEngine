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

//! A `LaneAgent` driven by a caller with no scheduler: `run_frame` is one
//! whole frame of its place — the wakes waiting answered, the frame start,
//! the installed layer's run, and the wakes that run raised answered.

use std::sync::Arc;

use khora_agents::lane_agent::{lane_agent, run_frame, LaneAgent};
use khora_core::agent::gorna::protocol::Report;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::Agent;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::io::FrameId;
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime};

use super::fixtures::{installed_row, runtime, wakes_of, Plan, Recorded, LADDER};

/// One frame of `agent` through `run_frame`, committing into a fresh deck:
/// the report it returned and the deck.
fn one_frame(
    agent: &mut LaneAgent,
    frame: u64,
    runtime: &Arc<Runtime>,
) -> (Option<Report>, OutputDeck) {
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(frame));
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    let id = agent.id();
    let report = {
        let mut context =
            EngineContext::for_agent(Arc::clone(runtime), &bus, &mut deck, &permit, Some(id));
        run_frame(agent, &mut context)
    };
    (report, deck)
}

/// **`run_frame` drives one frame without the scheduler.** The first call
/// answers the first-frame wake: the agent installs its default row (the
/// ladder's cheapest level, nothing being lent), and the layer runs at once —
/// its outputs are in the deck and its report names that row. No wake is
/// left waiting. A second call runs the installed layer again: its second
/// record.
#[test]
fn run_frame_drives_one_frame_without_the_scheduler() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    let runtime = runtime(1, Plan::default());

    let (report, deck) = one_frame(&mut agent, 1, &runtime);
    assert_eq!(
        report.map(|report| report.row),
        Some(StrategyId::LowPower),
        "the first frame runs the default row and returns its report"
    );
    assert_eq!(
        deck.get::<Recorded>(),
        Some(&Recorded {
            lane: "ladder",
            level: 0,
            record: 1,
        }),
        "the first frame's run committed its outputs"
    );
    assert_eq!(installed_row(&agent), Some(StrategyId::LowPower));
    assert!(
        wakes_of(&agent).is_empty(),
        "the frame answered every wake: {:?}",
        wakes_of(&agent)
    );

    let (report, deck) = one_frame(&mut agent, 2, &runtime);
    assert_eq!(
        report.map(|report| report.row),
        Some(StrategyId::LowPower),
        "the second frame runs the installed row again"
    );
    assert_eq!(
        deck.get::<Recorded>(),
        Some(&Recorded {
            lane: "ladder",
            level: 0,
            record: 2,
        }),
        "the second frame's run is the layer's second record"
    );
}
