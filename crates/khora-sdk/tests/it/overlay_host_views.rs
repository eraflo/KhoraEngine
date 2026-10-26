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

//! The engine boots with the overlays' Views already in its `HostViews`, each
//! empty: a host that never touches the overlays draws none, and one that does
//! sets them through the same handle.

use std::sync::Arc;

use khora_sdk::khora_core::lane::{HostViews, LaneBus};
use khora_sdk::khora_core::Runtime;
use khora_sdk::khora_data::render::{GizmoView, GridView, WireframeView};

/// An application that sets no overlay.
struct Bare;

impl khora_sdk::AgentProvider for Bare {
    fn register_agents(&self, _dcc: &khora_sdk::DccService, _runtime: &mut Runtime) {}
}
impl khora_sdk::PhaseProvider for Bare {}
impl khora_sdk::EngineApp for Bare {
    fn window_config() -> khora_sdk::WindowConfig {
        khora_sdk::WindowConfig::default()
    }
    fn new() -> Self {
        Bare
    }
    fn setup(&mut self, _world: &mut khora_sdk::GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut khora_sdk::GameWorld, _inputs: &[khora_sdk::InputEvent]) {}
}

/// **The engine publishes empty overlay Views.** After boot the runtime holds
/// the `HostViews` the host writes the overlays through; a frame's bus
/// receives a disabled grid, a gizmo View with no line and a disabled
/// wireframe from it.
#[test]
fn the_engine_publishes_empty_overlay_views() {
    let mut engine = khora_sdk::EngineCore::<Bare>::new();
    engine.bootstrap(Bare, Runtime::new());

    let host_views = engine
        .runtime()
        .resources
        .get::<Arc<HostViews>>()
        .expect("the engine boots with the host's Views");
    let mut bus = LaneBus::new();
    host_views.publish_into(&mut bus);

    let grid = bus.get::<GridView>().expect("a grid View");
    assert!(!grid.0.enabled, "the grid is off by default");
    let gizmo = bus.get::<GizmoView>().expect("a gizmo View");
    assert!(gizmo.0.is_empty(), "no gizmo line by default");
    let wireframe = bus.get::<WireframeView>().expect("a wireframe View");
    assert!(!wireframe.0.enabled, "the wireframe is off by default");
}
