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

//! The engine submits the frame's passes in the order their registrations
//! declare — shadows, the opaque scene, the sky, the blended scene, the UI,
//! the overlays — whatever order the agents recorded them in.

use std::any::TypeId;
use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_sdk::khora_core::agent::{Agent, AgentAccess, Contention};
use khora_sdk::khora_core::renderer::api::command::CommandBufferId;
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::khora_core::{EngineContext, Runtime};
use khora_sdk::khora_data::render::{
    registered_pass_order, OverlayPassSlot, PassContribution, PassDescriptor, ScenePassSlot,
    ShadowPassSlot, SkyboxPassSlot, TransparentPassSlot, UiPassSlot,
};
use khora_sdk::{ExecutionPhase, ExecutionTiming};

use super::render_doubles::{install_renderer, QueueLog, RecordingRenderer};

/// The six slots, in the order the frame must submit them.
fn declared_order() -> [TypeId; 6] {
    [
        TypeId::of::<ShadowPassSlot>(),
        TypeId::of::<ScenePassSlot>(),
        TypeId::of::<SkyboxPassSlot>(),
        TypeId::of::<TransparentPassSlot>(),
        TypeId::of::<UiPassSlot>(),
        TypeId::of::<OverlayPassSlot>(),
    ]
}

/// A contribution of a pass that declares no resource, so the frame graph
/// keeps the order it is handed.
fn pass(name: &'static str, buffer: u64) -> Option<PassContribution> {
    Some(PassContribution {
        descriptor: PassDescriptor::new(name),
        command_buffer: CommandBufferId(buffer),
    })
}

/// Records one pass into each of the six slots, overlay first and shadows
/// last: the reverse of the order they must reach the device in.
struct EverySlot;

impl Agent for EverySlot {
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
        ctx.deck.slot::<OverlayPassSlot>().0 = pass("OverlayPass", 6);
        ctx.deck.slot::<UiPassSlot>().0 = pass("UiPass", 5);
        ctx.deck.slot::<TransparentPassSlot>().0 = pass("TransparentPass", 4);
        ctx.deck.slot::<SkyboxPassSlot>().0 = pass("SkyboxPass", 3);
        ctx.deck.slot::<ScenePassSlot>().0 = pass("ScenePass", 2);
        ctx.deck.slot::<ShadowPassSlot>().0 = pass("ShadowPass", 1);
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

/// An application whose only agent records a pass into every slot.
struct Recorder;

impl khora_sdk::AgentProvider for Recorder {
    fn register_agents(&self, dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(EverySlot)), 1.0);
    }
}
impl khora_sdk::PhaseProvider for Recorder {}
impl khora_sdk::EngineApp for Recorder {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        Recorder
    }
    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **Passes reach the device in their declared order.** An agent records the
/// six passes overlay first; the frame submits them shadows, scene, sky,
/// transparent, UI, overlay — the shadow pass included, through the frame
/// graph like the others.
#[test]
fn passes_submit_in_declared_order() {
    let device = Arc::new(QueueLog::default());
    let mut runtime = Runtime::new();
    let (renderer, _prepared) = RecordingRenderer::into_target(800, 600);
    install_renderer(&mut runtime, renderer);
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);

    let mut engine = khora_sdk::EngineCore::<Recorder>::new();
    engine.bootstrap(Recorder, runtime);
    let frame_runtime = Arc::clone(engine.runtime());
    engine.tick_with_runtime(frame_runtime);

    let recorded: Vec<CommandBufferId> = device
        .submitted()
        .into_iter()
        .filter(|buffer| (1..=6).contains(&buffer.0))
        .collect();
    assert_eq!(
        recorded,
        (1..=6).map(CommandBufferId).collect::<Vec<_>>(),
        "the passes reach the device as shadow, scene, skybox, transparent, ui, overlay"
    );
}

/// The registered `after` edges give exactly that order, and no other pass
/// slot is registered beside the six.
#[test]
fn the_registered_pass_order_runs_from_shadows_to_overlays() {
    let order = match registered_pass_order() {
        Ok(order) => order,
        Err(refusal) => panic!("the shipped pass order has a cycle: {refusal}"),
    };
    let slots: Vec<TypeId> = order.iter().map(|pass| (pass.type_id)()).collect();
    let names: Vec<&str> = order.iter().map(|pass| pass.name).collect();
    assert_eq!(
        slots,
        declared_order(),
        "shadow → scene → skybox → transparent → ui → overlay, got {names:?}"
    );
}

/// Records the six passes overlay first, each with the resources the shipped
/// agent declares for it (`ShadowAgent`, `RenderAgent` with a shadow atlas,
/// `SkyboxAgent`, `UiAgent`, `OverlayAgent`).
struct EverySlotWithItsResources;

impl Agent for EverySlotWithItsResources {
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
        use khora_sdk::khora_data::render::ResourceId;
        let contribution = |descriptor: PassDescriptor, buffer: u64| {
            Some(PassContribution {
                descriptor,
                command_buffer: CommandBufferId(buffer),
            })
        };
        ctx.deck.slot::<OverlayPassSlot>().0 = contribution(
            PassDescriptor::new("OverlayPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth),
            6,
        );
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
        ctx.deck.slot::<SkyboxPassSlot>().0 = contribution(
            PassDescriptor::new("SkyboxPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth),
            3,
        );
        ctx.deck.slot::<ScenePassSlot>().0 = contribution(
            PassDescriptor::new("ScenePass")
                .writes(ResourceId::Color)
                .writes(ResourceId::Depth)
                .reads(ResourceId::ShadowAtlas),
            2,
        );
        ctx.deck.slot::<ShadowPassSlot>().0 = contribution(
            PassDescriptor::new("ShadowPass").writes(ResourceId::ShadowAtlas),
            1,
        );
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

/// An application whose only agent records every pass with its resources.
struct ResourceRecorder;

impl khora_sdk::AgentProvider for ResourceRecorder {
    fn register_agents(&self, dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {
        dcc.register_agent(Arc::new(Mutex::new(EverySlotWithItsResources)), 1.0);
    }
}
impl khora_sdk::PhaseProvider for ResourceRecorder {}
impl khora_sdk::EngineApp for ResourceRecorder {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        ResourceRecorder
    }
    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **The declared order survives the passes' own resources.** With the reads
/// and writes the shipped agents declare, the frame graph's sort must still
/// submit the UI before the overlays (`OverlayPassSlot` is registered after
/// `UiPassSlot`): the overlay pass reads only the depth, so nothing but the
/// registered order puts it after the UI.
#[test]
fn passes_with_their_resources_submit_in_declared_order() {
    let device = Arc::new(QueueLog::default());
    let mut runtime = Runtime::new();
    let (renderer, _prepared) = RecordingRenderer::into_target(800, 600);
    install_renderer(&mut runtime, renderer);
    runtime
        .backends
        .insert(Arc::clone(&device) as Arc<dyn GraphicsDevice>);

    let mut engine = khora_sdk::EngineCore::<ResourceRecorder>::new();
    engine.bootstrap(ResourceRecorder, runtime);
    let frame_runtime = Arc::clone(engine.runtime());
    engine.tick_with_runtime(frame_runtime);

    let recorded: Vec<CommandBufferId> = device
        .submitted()
        .into_iter()
        .filter(|buffer| (1..=6).contains(&buffer.0))
        .collect();
    assert_eq!(
        recorded,
        (1..=6).map(CommandBufferId).collect::<Vec<_>>(),
        "the passes reach the device as shadow, scene, skybox, transparent, ui, overlay"
    );
}
