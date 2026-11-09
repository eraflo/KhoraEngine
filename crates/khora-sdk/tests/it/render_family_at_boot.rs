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

//! The engine boots the scene renderer as the render family: the `LaneAgent`
//! built from the registration `khora-agents` submits, offering as
//! `renderer` — not a hand-written agent the SDK registers — with or without
//! a graphics device, and with Forward+ on its menu only over a device that
//! can run it.

use std::sync::Arc;

use khora_sdk::khora_core::agent::gorna::{AgentId, StrategyId};
use khora_sdk::khora_core::agent::ExecutionPhase;
use khora_sdk::khora_core::gpu::DeviceCaps;
use khora_sdk::khora_core::renderer::GraphicsDevice;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider,
    Runtime, WindowConfig,
};

use crate::render_doubles::QueueLog;

/// A game registering no agent of its own.
struct Game;

impl AgentProvider for Game {
    fn register_agents(&self, _dcc: &DccService, _runtime: &mut Runtime) {}
}
impl PhaseProvider for Game {}
impl EngineApp for Game {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Game
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// The renderer's name.
const RENDERER: AgentId = AgentId::new("renderer");

/// The strategies the `renderer` agent of `engine` offers, sorted by name.
fn renderer_menu(engine: &EngineCore<Game>) -> Vec<StrategyId> {
    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    let renderer = registry
        .iter()
        .filter(|agent| {
            agent.lock().is_ok_and(|agent| {
                agent
                    .execution_timing()
                    .allowed_phases
                    .contains(&ExecutionPhase::OUTPUT)
            })
        })
        .find(|agent| agent.lock().is_ok_and(|agent| agent.id() == RENDERER))
        .expect("the renderer is registered in OUTPUT");
    let mut renderer = renderer.lock().expect("the renderer's lock");
    let mut menu: Vec<StrategyId> = renderer
        .offer(&crate::protocol::request())
        .rows
        .iter()
        .map(|row| row.id)
        .collect();
    menu.sort_by_key(|id| format!("{id:?}"));
    menu
}

/// `ids`, sorted by name.
fn by_name(mut ids: Vec<StrategyId>) -> Vec<StrategyId> {
    ids.sort_by_key(|id| format!("{id:?}"));
    ids
}

/// **A headless boot registers the render family as the renderer.** With no
/// graphics device and no pipeline system in the runtime, the engine boots,
/// and the agent offering as `renderer` is the family's: it comes from
/// the module its registration was written in, and offers the unlit and the
/// forward lanes — no PBR row, and no Forward+ without a device to run it.
#[test]
fn a_headless_boot_registers_the_render_family_as_the_renderer() {
    let mut engine = EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    {
        let dcc = engine.dcc().expect("bootstrapped");
        let registry = dcc.agent_registry().lock().expect("the registry lock");
        assert_eq!(
            registry.origin(RENDERER),
            Some("khora_agents::render_agent::agent"),
            "the renderer is the family registered in `render_agent/agent.rs`"
        );
    }
    assert_eq!(
        renderer_menu(&engine),
        by_name(vec![StrategyId::LowPower, StrategyId::Balanced]),
        "the unlit and forward lanes, headless"
    );
}

/// **Over a device that can run it, Forward+ is on the renderer's menu.** A
/// device with storage buffers and compute shaders keeps the three lanes; a
/// device with storage buffers but no compute does not keep Forward+.
#[test]
fn forward_plus_is_on_the_renderers_menu_over_a_capable_device() {
    let boot_on = |caps: DeviceCaps| {
        let mut runtime = Runtime::new();
        runtime
            .backends
            .insert(Arc::new(QueueLog::with_caps(caps)) as Arc<dyn GraphicsDevice>);
        let mut engine = EngineCore::<Game>::new();
        engine.bootstrap(Game, runtime);
        renderer_menu(&engine)
    };

    assert_eq!(
        boot_on(DeviceCaps {
            storage_buffers: true,
            compute: true,
        }),
        by_name(vec![
            StrategyId::LowPower,
            StrategyId::Balanced,
            StrategyId::HighPerformance,
        ])
    );
    assert_eq!(
        boot_on(DeviceCaps {
            storage_buffers: true,
            compute: false,
        }),
        by_name(vec![StrategyId::LowPower, StrategyId::Balanced]),
        "no compute, no Forward+"
    );
}
