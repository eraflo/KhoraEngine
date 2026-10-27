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

//! What mixing costs, as the lane quotes it on the frame's real inputs: the
//! work of the voices a level mixes, on the host, and nothing elsewhere.

use std::time::Duration;

use khora_core::lane::cost::{Cost, PerLink};

use super::fixtures::{constant, view_of, Mixer, Voice, EIGHT_VOICES};

/// The quote of every level, cheapest first, for `sources` playing sources.
fn quotes(sources: usize) -> Vec<Cost> {
    let tone = constant(0.01, 4096);
    let voices: Vec<Voice> = (0..sources).map(|_| Voice::playing(&tone, 0.001)).collect();
    let mixed = Mixer::mono().mix(EIGHT_VOICES, view_of(None, &voices));
    mixed.recorded();
    assert_eq!(mixed.run.costs.len(), 3, "one quote per level");
    mixed.run.costs
}

/// Once the sources outnumber a level's cap, a level mixing more voices is
/// quoted dearer.
#[test]
fn mixing_more_voices_costs_more_once_the_cap_binds() {
    let costs = quotes(200);

    assert!(
        costs[0].time.cpu < costs[1].time.cpu && costs[1].time.cpu < costs[2].time.cpu,
        "8, 32 and 128 voices of 200 sources: {:?}",
        costs.iter().map(|c| c.time.cpu).collect::<Vec<_>>()
    );
}

/// With fewer sources than the smallest cap every level mixes the same voices,
/// so every level is quoted the same.
#[test]
fn levels_cost_the_same_when_no_voice_is_virtual() {
    let costs = quotes(4);

    assert!(
        costs[0].time.cpu > Duration::ZERO,
        "mixing four voices costs something"
    );
    assert!(
        costs.iter().all(|cost| cost.time.cpu == costs[0].time.cpu),
        "four sources, every level mixes all four: {:?}",
        costs.iter().map(|c| c.time.cpu).collect::<Vec<_>>()
    );
}

/// The quote follows the frame: more sources to mix at one level cost more.
#[test]
fn the_cost_follows_the_voices_a_level_mixes() {
    let fewer = quotes(4);
    let more = quotes(6);

    assert!(
        more[0].time.cpu > fewer[0].time.cpu,
        "six voices ({:?}) cost more than four ({:?})",
        more[0].time.cpu,
        fewer[0].time.cpu
    );
}

/// Mixing runs on the host: no time on the graphics device, nothing sent over
/// a link.
#[test]
fn audio_costs_host_time_only() {
    for (level, cost) in quotes(40).iter().enumerate() {
        assert_eq!(cost.time.gpu, Duration::ZERO, "level {level}: no GPU time");
        assert_eq!(cost.time.npu, Duration::ZERO, "level {level}: no NPU time");
        assert_eq!(
            cost.transfer,
            PerLink::default(),
            "level {level}: no transfer"
        );
    }
}
