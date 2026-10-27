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

//! Frames through the shadow agent, as the scheduler runs them: the budget
//! picks a level of the one shadows lane, and the agent commits the frame's
//! shadows and its pass — or nothing at all.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use khora_core::agent::gorna::{ResourceBudget, StrategyId};
use khora_core::agent::Agent;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::shadow::{ShadowEntry, ShadowFrame};
use khora_data::render::ShadowPassSlot;

use super::fixtures::{bus_with_a_sun, run_once, shadow_agent, shadows_runtime, SubmitLog};

fn budget(strategy_id: StrategyId) -> ResourceBudget {
    ResourceBudget {
        strategy_id,
        time_limit: Duration::from_millis(8),
        memory_limit: None,
        extra_params: HashMap::new(),
    }
}

/// **A budget picks the level of the shadows lane.** Each of the three
/// strategies GORNA may hand the agent is reported back as its current one,
/// and the frame it runs fills atlases at that level's resolution: 512/128,
/// 1024/256, 2048/512 texels.
#[test]
fn a_budget_picks_the_shadow_level() {
    for (strategy, sides) in [
        (StrategyId::LowPower, [128, 512]),
        (StrategyId::Balanced, [256, 1024]),
        (StrategyId::HighPerformance, [512, 2048]),
    ] {
        let device = Arc::new(SubmitLog::new());
        let runtime = Arc::new(shadows_runtime(&device));
        let mut agent = shadow_agent();
        agent.apply_budget(budget(strategy));
        assert_eq!(
            agent.report_status().current_strategy,
            strategy,
            "the agent runs the row GORNA picked"
        );

        let mut deck = OutputDeck::new();
        run_once(&mut agent, &runtime, &bus_with_a_sun(), &mut deck);

        assert_eq!(
            device.atlas_sides(),
            sides,
            "{strategy:?} fills its own atlases"
        );
    }
}

/// **A frame through the agent publishes the shadows for the lit lanes**: the
/// deck holds the `ShadowFrame`, naming its atlases and the sun's layer, and
/// the `ShadowPassSlot` holding the buffer the agent's one encoder finished
/// into.
#[test]
fn a_frame_through_the_agent_publishes_the_shadows_and_their_pass() {
    let device = Arc::new(SubmitLog::new());
    let runtime = Arc::new(shadows_runtime(&device));
    let mut agent = shadow_agent();
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &bus_with_a_sun(), &mut deck);

    let frame = deck
        .get::<ShadowFrame>()
        .expect("the agent committed the shadow frame");
    assert!(frame.bindings().is_some(), "the frame names its atlases");
    assert!(
        matches!(
            frame.entry(0),
            Some(ShadowEntry::Atlas2D { atlas_index: 0, .. })
        ),
        "the sun has the first layer: {:?}",
        frame.entry(0)
    );
    let in_slot = deck
        .get::<ShadowPassSlot>()
        .and_then(|slot| slot.0.as_ref())
        .map(|pass| pass.command_buffer);
    assert_eq!(device.opened().len(), 1, "one encoder a frame");
    assert_eq!(
        device.finished(),
        in_slot.into_iter().collect::<Vec<_>>(),
        "the one buffer finished is the one in the slot"
    );
    assert!(device.submitted().is_empty(), "the engine submits passes");
}

/// **A frame whose pass cannot be closed commits nothing.** The encoder does
/// not finish, so the pass output is refused: neither the pass slot nor the
/// shadow frame reaches the deck, and the lit lanes draw without shadows
/// rather than sampling an atlas no pass filled.
#[test]
fn a_frame_whose_pass_cannot_close_commits_nothing() {
    let device = Arc::new(SubmitLog::unfinishable());
    let runtime = Arc::new(shadows_runtime(&device));
    let mut agent = shadow_agent();
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &bus_with_a_sun(), &mut deck);

    assert!(
        !deck.contains::<ShadowPassSlot>(),
        "no pass reaches the slot"
    );
    assert!(
        !deck.contains::<ShadowFrame>(),
        "no shadow frame is committed: {:?}",
        deck.get::<ShadowFrame>()
    );
    assert!(
        !agent.report_status().message.is_empty(),
        "the agent says why"
    );
}

/// **Without a device the shadows family records nothing, and says why.** A
/// headless engine holds the asset store but no device and no pipeline
/// system: each frame commits neither the pass nor the shadow frame, and the
/// agent's status names the missing device.
#[test]
fn a_headless_frame_commits_no_shadows_and_names_the_device() {
    let mut runtime = khora_core::Runtime::new();
    runtime.resources.insert(khora_data::AssetStore::new());
    let runtime = Arc::new(runtime);
    let mut agent = shadow_agent();

    for _ in 0..3 {
        let mut deck = OutputDeck::new();
        run_once(&mut agent, &runtime, &bus_with_a_sun(), &mut deck);
        assert!(
            !deck.contains::<ShadowPassSlot>(),
            "no pass without a device"
        );
        assert!(
            !deck.contains::<ShadowFrame>(),
            "no shadow frame without a device"
        );
    }
    let message = agent.report_status().message;
    assert!(
        message.contains("GraphicsDevice"),
        "the status names the device: {message:?}"
    );
}

/// **Without a pipeline system the shadows family builds nothing on the
/// device.** The device is there, the pipeline system is not: the prepare
/// fails before any atlas is made, every frame, and the status names the
/// pipeline system.
#[test]
fn a_frame_without_a_pipeline_system_builds_nothing() {
    let device = Arc::new(SubmitLog::new());
    let mut runtime = khora_core::Runtime::new();
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn khora_core::renderer::GraphicsDevice>);
    runtime.resources.insert(khora_data::AssetStore::new());
    let runtime = Arc::new(runtime);
    let mut agent = shadow_agent();

    for _ in 0..3 {
        let mut deck = OutputDeck::new();
        run_once(&mut agent, &runtime, &bus_with_a_sun(), &mut deck);
        assert!(deck.is_empty(), "nothing committed: {deck:?}");
    }
    assert!(
        device.atlas_sides().is_empty(),
        "no atlas built: {:?}",
        device.atlas_sides()
    );
    assert!(device.opened().is_empty(), "no encoder opened");
    let message = agent.report_status().message;
    assert!(
        message.contains("PipelineSystem"),
        "the status names the pipeline system: {message:?}"
    );
}
