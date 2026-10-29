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

//! The UI as the engine runs it: one agent, built from the UI family's
//! registration, in every mode; and its pass, composited where it always
//! was — after the transparent pass, before the grid.

use std::any::TypeId;

use khora_agents::lane_agent::LaneAgent;
use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::agent::EngineMode;
use khora_sdk::khora_core::lane::io::PassSlot;
use khora_sdk::khora_core::Runtime;
use khora_sdk::khora_data::render::{
    registered_pass_order, GridPassSlot, PassDescriptor, ResourceId, TransparentPassSlot,
    UiPassSlot,
};
use khora_sdk::ExecutionPhase;

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

/// **The engine boots the UI family as one lane agent**, in `OUTPUT`, in
/// play and in a tool's mode alike: one `Ui` agent, the `LaneAgent` built
/// from the family's registration, and no other `Ui` agent beside it. It
/// boots on an application's empty runtime, as every other family does.
#[test]
fn the_engine_boots_the_ui_family_as_one_lane_agent() {
    let mut engine = khora_sdk::EngineCore::<Game>::new();
    engine.bootstrap(Game, Runtime::new());

    let dcc = engine.dcc().expect("bootstrapped");
    let registry = dcc.agent_registry().lock().expect("the registry lock");
    for mode in [EngineMode::Playing, EngineMode::Custom("tool".to_owned())] {
        let ui: Vec<bool> = registry
            .collect_for_phase(ExecutionPhase::OUTPUT, &mode)
            .into_iter()
            .filter_map(|(agent, ..)| {
                agent
                    .lock()
                    .ok()
                    .filter(|agent| agent.id() == AgentId::Ui)
                    .map(|agent| agent.as_any().is::<LaneAgent>())
            })
            .collect();
        assert_eq!(
            ui,
            [true],
            "{mode:?}: one UI agent in OUTPUT, the lane agent of the UI family"
        );
    }
}

/// **The UI pass composites after the transparent pass and before the
/// grid**, as it always did, and its slot opens with the descriptor the old
/// agent declared: `UiPass`, reading and writing the colour.
#[test]
fn the_ui_pass_composites_after_the_transparent_pass_and_before_the_grid() {
    let order = match registered_pass_order() {
        Ok(order) => order,
        Err(refusal) => panic!("the shipped pass order has a cycle: {refusal}"),
    };
    let slots: Vec<TypeId> = order.iter().map(|pass| (pass.type_id)()).collect();
    let at = |ty: TypeId| {
        slots
            .iter()
            .position(|slot| *slot == ty)
            .expect("the slot is registered")
    };
    let ui = at(TypeId::of::<UiPassSlot>());
    assert_eq!(at(TypeId::of::<TransparentPassSlot>()) + 1, ui);
    assert_eq!(at(TypeId::of::<GridPassSlot>()), ui + 1);

    let PassDescriptor {
        name,
        reads,
        writes,
    } = UiPassSlot::descriptor();
    assert_eq!(name, "UiPass");
    assert_eq!(reads, [ResourceId::Color]);
    assert_eq!(writes, [ResourceId::Color]);
}

/// A text renderer of the application's own, drawing nothing.
struct AppText;

/// A layout of no size, from [`AppText`].
struct AppLayout;

impl khora_sdk::khora_core::renderer::api::text::TextLayout for AppLayout {
    fn size(&self) -> khora_sdk::khora_core::math::Vec2 {
        khora_sdk::khora_core::math::Vec2::ZERO
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl khora_sdk::TextRenderer for AppText {
    fn layout_text(
        &self,
        _text: &str,
        _face: khora_sdk::khora_core::renderer::api::text::FontFace<'_>,
        _font_size: f32,
        _max_width: Option<f32>,
    ) -> Box<dyn khora_sdk::khora_core::renderer::api::text::TextLayout> {
        Box::new(AppLayout)
    }

    fn queue_text(
        &self,
        _layout: &dyn khora_sdk::khora_core::renderer::api::text::TextLayout,
        _pos: khora_sdk::khora_core::math::Vec2,
        _color: khora_sdk::khora_core::math::Vec4,
        _z_index: i32,
    ) {
    }

    fn flush(
        &self,
        _device: &dyn khora_sdk::khora_core::renderer::GraphicsDevice,
        _encoder: &mut dyn khora_sdk::khora_core::renderer::traits::CommandEncoder,
        _color_target: &khora_sdk::khora_core::renderer::api::resource::TextureViewId,
        _target: khora_sdk::khora_core::math::Extent2D,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

/// **An application's own text renderer is kept.** The engine installs the
/// standard one only when the application brings none: booted over a
/// runtime holding the application's renderer, the engine's runtime holds
/// that very renderer, which the UI family's lane then queues its texts to.
#[test]
fn an_applications_own_text_renderer_is_kept() {
    use std::sync::Arc;

    let own: Arc<dyn khora_sdk::TextRenderer> = Arc::new(AppText);
    let mut runtime = Runtime::new();
    runtime.backends.insert(Arc::clone(&own));

    let mut engine = khora_sdk::EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);

    let installed = engine
        .runtime()
        .backends
        .get::<Arc<dyn khora_sdk::TextRenderer>>()
        .expect("a text renderer is installed");
    assert!(
        Arc::ptr_eq(installed, &own),
        "the application's own text renderer, not the engine's standard one"
    );
}
