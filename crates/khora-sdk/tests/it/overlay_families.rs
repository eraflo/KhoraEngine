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

//! The overlays as the engine runs them: three agents, one per family, and
//! three passes composited after the UI — the grid, then the wireframe, then
//! the gizmos — whatever order they were recorded in.

use std::any::TypeId;
use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_sdk::khora_core::agent::{Agent, AgentAccess, Contention, EngineMode};
use khora_sdk::khora_core::lane::io::PassSlot;
use khora_sdk::khora_core::renderer::api::command::CommandBufferId;
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::khora_core::{EngineContext, Runtime};
use khora_sdk::khora_data::render::{
    registered_pass_order, GizmoPassSlot, GridPassSlot, PassContribution, PassDescriptor,
    ResourceId, ScenePassSlot, ShadowPassSlot, SkyboxPassSlot, TransparentPassSlot, UiPassSlot,
    WireframePassSlot,
};
use khora_sdk::{ExecutionPhase, ExecutionTiming};

use super::render_doubles::{install_renderer, QueueLog, RecordingRenderer};

/// The eight pass slots, in the order the frame must submit them.
fn declared_order() -> [TypeId; 8] {
    [
        TypeId::of::<ShadowPassSlot>(),
        TypeId::of::<ScenePassSlot>(),
        TypeId::of::<SkyboxPassSlot>(),
        TypeId::of::<TransparentPassSlot>(),
        TypeId::of::<UiPassSlot>(),
        TypeId::of::<GridPassSlot>(),
        TypeId::of::<WireframePassSlot>(),
        TypeId::of::<GizmoPassSlot>(),
    ]
}

/// **The overlays composite grid, then wireframe, then gizmos.** The
/// registered `after` edges order the eight slots from the shadows to the
/// UI, then the three overlays in the order the old overlay agent drew them
/// into one pass; no other pass slot is registered beside them.
#[test]
fn the_overlays_composite_grid_then_wireframe_then_gizmos() {
    let order = match registered_pass_order() {
        Ok(order) => order,
        Err(refusal) => panic!("the shipped pass order has a cycle: {refusal}"),
    };
    let slots: Vec<TypeId> = order.iter().map(|pass| (pass.type_id)()).collect();
    let names: Vec<&str> = order.iter().map(|pass| pass.name).collect();
    assert_eq!(
        slots,
        declared_order(),
        "shadow → scene → skybox → transparent → ui → grid → wireframe → gizmo, got {names:?}"
    );
}

/// Records the eight passes gizmos first and shadows last — the reverse of
/// the order they must reach the device in — each with the descriptor its
/// slot opens with, or, for the slots no family closes yet, the one their
/// agent declares.
struct EveryPassReversed;

impl Agent for EveryPassReversed {
    fn id(&self) -> AgentId {
        AgentId::Asset
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: AgentId::Asset,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn access(&self) -> AgentAccess {
        AgentAccess::Isolated
    }
    fn contention(&self) -> Contention {
        Contention::none().writing_deck(declared_order())
    }
    fn execute(&mut self, ctx: &mut EngineContext<'_>) {
        let contribution = |descriptor: PassDescriptor, buffer: u64| {
            Some(PassContribution {
                descriptor,
                command_buffer: CommandBufferId(buffer),
            })
        };
        ctx.deck.slot::<GizmoPassSlot>().0 = contribution(GizmoPassSlot::descriptor(), 8);
        ctx.deck.slot::<WireframePassSlot>().0 = contribution(WireframePassSlot::descriptor(), 7);
        ctx.deck.slot::<GridPassSlot>().0 = contribution(GridPassSlot::descriptor(), 6);
        ctx.deck.slot::<UiPassSlot>().0 = contribution(
            PassDescriptor::new("UiPass")
                .reads(ResourceId::Color)
                .writes(ResourceId::Color),
            5,
        );
        ctx.deck.slot::<TransparentPassSlot>().0 = contribution(
            PassDescriptor::new("TransparentPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth),
            4,
        );
        ctx.deck.slot::<SkyboxPassSlot>().0 = contribution(SkyboxPassSlot::descriptor(), 3);
        ctx.deck.slot::<ScenePassSlot>().0 = contribution(
            PassDescriptor::new("ScenePass")
                .writes(ResourceId::Color)
                .writes(ResourceId::Depth)
                .reads(ResourceId::ShadowAtlas),
            2,
        );
        ctx.deck.slot::<ShadowPassSlot>().0 = contribution(ShadowPassSlot::descriptor(), 1);
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            ..ExecutionTiming::default()
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// An application whose only agent records every pass, reversed.
struct Reversed;

impl khora_sdk::AgentProvider for Reversed {
    fn register_agents(&self, dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(EveryPassReversed)), 1.0);
    }
}
impl khora_sdk::PhaseProvider for Reversed {}
impl khora_sdk::EngineApp for Reversed {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        Reversed
    }
    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **The overlay passes reach the device after the UI, grid then wireframe
/// then gizmos.** Recorded in reverse, with the resources their slots
/// declare — the grid and the wireframe read only the depth, the gizmos
/// nothing — the frame submits all eight in their registered order: nothing
/// but that order puts the gizmos after the UI.
#[test]
fn overlay_passes_submit_after_the_ui_in_compositing_order() {
    let device = Arc::new(QueueLog::default());
    let mut runtime = Runtime::new();
    let (renderer, _prepared) = RecordingRenderer::into_target(800, 600);
    install_renderer(&mut runtime, renderer);
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);

    let mut engine = khora_sdk::EngineCore::<Reversed>::new();
    engine.bootstrap(Reversed, runtime);
    let frame_runtime = Arc::clone(engine.runtime());
    engine.tick_with_runtime(frame_runtime);

    let recorded: Vec<CommandBufferId> = device
        .submitted()
        .into_iter()
        .filter(|buffer| (1..=8).contains(&buffer.0))
        .collect();
    assert_eq!(
        recorded,
        (1..=8).map(CommandBufferId).collect::<Vec<_>>(),
        "shadow, scene, skybox, transparent, ui, grid, wireframe, gizmo"
    );
}

/// A game registering no agent of its own.
struct Game;

impl khora_sdk::AgentProvider for Game {
    fn register_agents(&self, _dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {}
}
impl khora_sdk::PhaseProvider for Game {}
impl khora_sdk::EngineApp for Game {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        Game
    }
    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **The engine boots one agent per overlay**, in `OUTPUT`: the grid's, the
/// wireframe's and the gizmos', once each — and no agent holding all three.
#[test]
fn the_engine_boots_one_agent_per_overlay() {
    let mut engine = khora_sdk::EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    let output: Vec<AgentId> = registry
        .collect_for_phase(ExecutionPhase::OUTPUT, &EngineMode::Playing)
        .into_iter()
        .filter_map(|(agent, ..)| agent.lock().ok().map(|agent| agent.id()))
        .collect();
    for overlay in [AgentId::Grid, AgentId::Wireframe, AgentId::Gizmo] {
        assert_eq!(
            output.iter().filter(|id| **id == overlay).count(),
            1,
            "one {overlay} agent in OUTPUT: {output:?}"
        );
    }
    assert!(
        output.iter().all(|id| id.to_string() != "Overlay"),
        "the agent that held all three overlays is gone: {output:?}"
    );
}
