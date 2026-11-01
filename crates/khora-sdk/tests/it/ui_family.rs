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
//! registration, in every mode; its pass, composited where it always was —
//! after the transparent pass, before the grid; and the text backends it
//! draws through, installed by default and kept when the application brings
//! its own.

use std::any::TypeId;
use std::sync::Arc;

use khora_agents::agent::LaneAgent;
use khora_sdk::khora_core::agent::gorna::AgentId;
use khora_sdk::khora_core::agent::EngineMode;
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::khora_core::lane::io::PassSlot;
use khora_sdk::khora_core::ui::text::{
    FontError, FontFamily, GlyphImage, GlyphKey, GlyphRasterizer, ShapeError, ShapeRequest,
    ShapedText, TextShaper,
};
use khora_sdk::khora_core::Runtime;
use khora_sdk::khora_data::render::{
    registered_pass_order, GridPassSlot, PassDescriptor, ResourceId, TransparentPassSlot,
    UiPassSlot,
};
use khora_sdk::khora_data::ui::GlyphAtlas;
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

/// A shaper of the application's own, which knows no font.
struct AppShaper;

impl TextShaper for AppShaper {
    fn register_font(&self, _id: AssetUUID, _data: Arc<[u8]>) -> Result<FontFamily, FontError> {
        Err(FontError::Unreadable)
    }

    fn shape(&self, request: &ShapeRequest<'_>) -> Result<ShapedText, ShapeError> {
        Err(ShapeError::UnknownFont(request.font))
    }
}

/// A rasteriser of the application's own, which draws nothing.
struct AppRasterizer;

impl GlyphRasterizer for AppRasterizer {
    fn register_font(&self, _id: AssetUUID, _data: Arc<[u8]>) -> Result<(), FontError> {
        Err(FontError::Unreadable)
    }

    fn rasterize(&self, _key: &GlyphKey) -> Option<GlyphImage> {
        None
    }
}

/// The engine booted over `runtime`, as an application hands it over.
fn booted(runtime: Runtime) -> khora_sdk::EngineCore<Game> {
    let mut engine = khora_sdk::EngineCore::<Game>::new();
    engine.bootstrap(Game, runtime);
    engine
}

/// **The default install provides the shaper, the rasteriser and the glyph
/// atlas.** Booted over an application's empty runtime, the engine's runtime
/// holds a `TextShaper` and a `GlyphRasterizer` among its backends — what
/// `ui_text_layout` and `glyphs_on_gpu` need — and the `GlyphAtlas` the UI
/// lane draws from among its resources.
#[test]
fn the_default_install_provides_the_shaper_the_rasteriser_and_the_glyph_atlas() {
    let engine = booted(Runtime::new());
    let runtime = engine.runtime();

    assert!(
        runtime.backends.get::<Arc<dyn TextShaper>>().is_some(),
        "a text shaper is installed"
    );
    assert!(
        runtime.backends.get::<Arc<dyn GlyphRasterizer>>().is_some(),
        "a glyph rasteriser is installed"
    );
    assert!(
        runtime.resources.get::<Arc<GlyphAtlas>>().is_some(),
        "a glyph atlas is installed"
    );
}

/// **An application's own shaper is kept**, and the engine's rasteriser is
/// installed beside it: booted over a runtime holding the application's
/// shaper alone, the engine's runtime holds that very shaper, and a
/// rasteriser.
#[test]
fn an_applications_own_shaper_is_kept_and_the_rasteriser_installed_beside_it() {
    let own: Arc<dyn TextShaper> = Arc::new(AppShaper);
    let mut runtime = Runtime::new();
    runtime.backends.insert(Arc::clone(&own));

    let engine = booted(runtime);

    let installed = engine
        .runtime()
        .backends
        .get::<Arc<dyn TextShaper>>()
        .expect("a text shaper is installed");
    assert!(
        Arc::ptr_eq(installed, &own),
        "the application's own shaper, not the engine's"
    );
    assert!(
        engine
            .runtime()
            .backends
            .get::<Arc<dyn GlyphRasterizer>>()
            .is_some(),
        "and the engine's rasteriser beside it"
    );
}

/// **An application's own rasteriser is kept**, and the engine's shaper is
/// installed beside it.
#[test]
fn an_applications_own_rasteriser_is_kept_and_the_shaper_installed_beside_it() {
    let own: Arc<dyn GlyphRasterizer> = Arc::new(AppRasterizer);
    let mut runtime = Runtime::new();
    runtime.backends.insert(Arc::clone(&own));

    let engine = booted(runtime);

    let installed = engine
        .runtime()
        .backends
        .get::<Arc<dyn GlyphRasterizer>>()
        .expect("a glyph rasteriser is installed");
    assert!(
        Arc::ptr_eq(installed, &own),
        "the application's own rasteriser, not the engine's"
    );
    assert!(
        engine
            .runtime()
            .backends
            .get::<Arc<dyn TextShaper>>()
            .is_some(),
        "and the engine's shaper beside it"
    );
}
