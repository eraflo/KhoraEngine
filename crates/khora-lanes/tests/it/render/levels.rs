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

//! Each render lane offers one level, and Forward+ runs only on a device
//! that can cull its lights.

use khora_core::agent::gorna::StrategyId;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::level::Impact;

use super::fixtures::{Which, EVERY_LANE};

const MIB: u64 = 1024 * 1024;

/// **One level per lane, holding the memory the old render agent quoted.**
/// The unlit lane is `LowPower`, a difference a player sees, holding no
/// graphics memory of note; the forward lane is `Balanced`, the reference
/// image, holding 4 KiB; Forward+ is `HighPerformance`, the reference image,
/// holding 4 KiB and 8 MiB of light lists.
#[test]
fn each_render_lane_offers_one_level() {
    for (which, id, impact, gpu) in [
        (Which::Unlit, StrategyId::LowPower, Impact::Major, 0),
        (Which::Forward, StrategyId::Balanced, Impact::None, 4096),
        (
            Which::ForwardPlus,
            StrategyId::HighPerformance,
            Impact::None,
            4096 + 8 * MIB,
        ),
    ] {
        let lane = which.lane();
        let levels = lane.levels();
        assert_eq!(levels.len(), 1, "{which:?}: one level");
        assert_eq!(levels[0].id, id, "{which:?}");
        assert_eq!(levels[0].impact, impact, "{which:?}");
        assert_eq!(levels[0].memory.gpu, gpu, "{which:?}'s graphics memory");
    }
}

/// **Forward+ needs storage buffers and compute shaders**: its light lists
/// are storage buffers a compute pass fills. On a device lacking either it
/// is not supported.
#[test]
fn forward_plus_runs_only_with_storage_buffers_and_compute() {
    let lane = Which::ForwardPlus.lane();
    for (storage_buffers, compute, supported) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let caps = DeviceCaps {
            storage_buffers,
            compute,
        };
        assert_eq!(lane.supports(&caps), supported, "{caps:?}");
    }
}

/// The unlit and forward lanes run on any device, so the family always has
/// a lane to draw with.
#[test]
fn the_unlit_and_forward_lanes_run_on_any_device() {
    for which in EVERY_LANE
        .into_iter()
        .filter(|which| *which != Which::ForwardPlus)
    {
        assert!(
            which.lane().supports(&DeviceCaps::default()),
            "{which:?} runs without storage buffers or compute"
        );
    }
}
