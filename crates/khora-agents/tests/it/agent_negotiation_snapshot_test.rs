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

//! What every agent offers GORNA, pinned: the rows, in order, with their
//! VRAM quote, for a request before any frame.
//!
//! Reorganising where lanes live must not change what the arbiter is offered:
//! each family offers one row per level of its lanes, so a lane that goes
//! missing drops a strategy here. Timings are left out: they come from cost
//! estimators, which are free to evolve.
//!
//! Nothing here needs a GPU: `offer` runs on the DCC thread against a stub
//! scene.

use khora_agents::agent::lane_agent;
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::Agent;
use khora_core::gpu::DeviceCaps;

use crate::audio_family::audio_registration;
use crate::overlay_families::fixtures::{
    agent_of, gizmo_registration, grid_registration, wireframe_registration,
};
use crate::physics_family::fixtures::physics_agent;
use crate::protocol::request;
use crate::render_family::fixtures::{render_agent, EVERY_CAP};
use crate::script_family::fixtures::script_agent;
use crate::shadows_family::fixtures::{shadow_agent, LEVEL_MEMORY};
use crate::skybox_family::fixtures::skybox_agent;
use crate::ui_family::fixtures::ui_agent;

const MIB: u64 = 1024 * 1024;

/// The agent's id and its offer as `(row, graphics memory)`, in the order
/// the agent listed them.
fn offer(mut agent: impl Agent) -> (AgentId, Vec<(StrategyId, u64)>) {
    let offer = agent.offer(&request());
    let rows = offer
        .rows
        .iter()
        .map(|row| (row.id, row.cost.memory.gpu))
        .collect();
    (agent.id(), rows)
}

/// The render family's agent on a device with storage buffers and compute,
/// before any frame: one row per lane — the unlit, the forward and the
/// Forward+ lane — each quoting the graphics memory its level holds. No
/// `Custom(1)`: the PBR lane is gone.
#[test]
fn render_agent_offers_one_strategy_per_lane() {
    let (id, mut strategies) = offer(render_agent(&EVERY_CAP));
    strategies.sort_by_key(|(id, _)| format!("{id:?}"));
    assert_eq!(
        (id, strategies),
        (
            AgentId::new("renderer"),
            vec![
                (StrategyId::Balanced, 4096),
                (StrategyId::HighPerformance, 4096 + 8 * MIB),
                (StrategyId::LowPower, 0),
            ]
        )
    );
}

/// The shadows family's agent, before any frame: one row per level of its
/// one lane, cheapest first, each quoting the graphics memory of its atlases —
/// 5.5, 22 and 88 MiB.
#[test]
fn shadow_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(shadow_agent()),
        (
            AgentId::new("shadows"),
            vec![
                (StrategyId::LowPower, LEVEL_MEMORY[0]),
                (StrategyId::Balanced, LEVEL_MEMORY[1]),
                (StrategyId::HighPerformance, LEVEL_MEMORY[2]),
            ]
        )
    );
}

/// The three overlay families' agents, before any frame: one `Balanced` row
/// each, quoting the graphics memory of its one level — the grid's 80-byte
/// camera block; the wireframe's camera and 32-byte style blocks plus its
/// model ring's first chunk in each of the 2 frames in flight (1000 blocks of
/// 256 bytes), since a level quotes its whole first build before it runs;
/// the gizmos' camera block and the storage for 4096 lines of 48 bytes.
#[test]
fn overlay_agents_offer_one_flat_strategy_each() {
    assert_eq!(
        offer(agent_of(grid_registration())),
        (AgentId::new("grid"), vec![(StrategyId::Balanced, 80)])
    );
    assert_eq!(
        offer(agent_of(wireframe_registration())),
        (
            AgentId::new("wireframe"),
            vec![(StrategyId::Balanced, 80 + 32 + 2 * 1000 * 256)]
        )
    );
    assert_eq!(
        offer(agent_of(gizmo_registration())),
        (
            AgentId::new("gizmo"),
            vec![(StrategyId::Balanced, 80 + 4096 * 48)]
        )
    );
}

/// The skybox family's agent, before any frame: one `Balanced` row, quoting
/// the graphics memory of its one level — the 80-byte uniform block.
#[test]
fn skybox_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(skybox_agent()),
        (AgentId::new("skybox"), vec![(StrategyId::Balanced, 80)])
    );
}

/// The UI family's agent, before any frame: one `Balanced` row, quoting the
/// graphics memory of its one level — the 64-byte projection block, the
/// storage for 1024 node instances and the storage for 1024 glyph instances,
/// 64 bytes each.
#[test]
fn ui_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(ui_agent()),
        (
            AgentId::new("ui"),
            vec![(StrategyId::Balanced, 64 + 1024 * 64 + 1024 * 64)]
        )
    );
}

/// The physics family's agent: one level, which keeps the result, and no
/// VRAM. Its step is a game setting, so GORNA has no step to choose.
#[test]
fn physics_agent_offers_one_flat_strategy() {
    assert_eq!(
        offer(physics_agent()),
        (AgentId::new("physics"), vec![(StrategyId::Balanced, 0)])
    );
}

/// The audio family's agent, before any frame: one row per voice level, no
/// VRAM.
#[test]
fn audio_agent_offers_three_tiers_from_low_power_up() {
    assert_eq!(
        offer(lane_agent(audio_registration(), &DeviceCaps::default())),
        (
            AgentId::new("audio"),
            vec![
                (StrategyId::LowPower, 0),
                (StrategyId::Balanced, 0),
                (StrategyId::HighPerformance, 0),
            ]
        )
    );
}

/// The script family's agent, before any frame: two rows, cheapest first —
/// its critical level, `LowPower`, and its whole level, `Balanced` — and no
/// VRAM.
#[test]
fn script_agent_offers_a_critical_row_then_a_whole_one() {
    assert_eq!(
        offer(script_agent()),
        (
            AgentId::new("script"),
            vec![(StrategyId::LowPower, 0), (StrategyId::Balanced, 0)]
        )
    );
}
