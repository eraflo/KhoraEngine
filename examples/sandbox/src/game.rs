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

//! The sandbox game: the scene it builds and what it updates each frame.

use super::assets::{register_checker_texture, register_environment_map, ENV_ASSET_NAME};
use super::player::PlayerController;
use khora_sdk::khora_core::platform::InputMap;
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::{
    AgentProvider, DccService, EngineApp, GameWorld, InputEvent, PhaseProvider, Runtime,
    WindowConfig,
};
use std::sync::{Arc, Mutex};

/// A simple game demonstrating the Khora SDK.
pub(super) struct SandboxGame {
    frame_count: u64,
    player: Option<khora_sdk::prelude::ecs::EntityId>,
    controller: PlayerController,
    /// Cached handle to the engine's InputMap. Stored at `setup` time so
    /// `update` (which doesn't receive `&ServiceRegistry`) can still query
    /// actions each frame.
    input_map: Option<Arc<Mutex<InputMap>>>,
    /// Cached handle to the engine's per-frame [`Time`](khora_sdk::prelude::Time) resource. Cached at
    /// `setup` time so `update` can read the real frame delta (the scheduler
    /// publishes it each frame) instead of a hardcoded constant.
    time: Option<khora_sdk::prelude::SharedTime>,
}

impl EngineApp for SandboxGame {
    fn window_config() -> WindowConfig {
        WindowConfig {
            title: "Khora Sandbox".to_owned(),
            ..WindowConfig::default()
        }
    }

    fn new() -> Self {
        log::info!("SandboxGame: Initializing...");
        Self {
            frame_count: 0,
            player: None,
            controller: PlayerController::new(),
            input_map: None,
            time: None,
        }
    }

    fn setup(&mut self, world: &mut GameWorld, runtime: &Runtime) {
        // Cache the InputMap handle and register the player movement bindings.
        if let Some(map_arc) = runtime.resources.get::<Arc<Mutex<InputMap>>>() {
            if let Ok(mut map) = map_arc.lock() {
                PlayerController::bind_actions(&mut map);
            }
            self.input_map = Some(map_arc.clone());
        }

        // Cache the per-frame Time handle so `update` reads the real frame
        // delta instead of a hardcoded step.
        self.time = runtime
            .resources
            .get::<khora_sdk::prelude::SharedTime>()
            .cloned();

        let camera = khora_sdk::prelude::ecs::Camera::new_perspective(
            std::f32::consts::FRAC_PI_4,
            16.0 / 9.0,
            0.1,
            1000.0,
        );
        self.player = Some(
            khora_sdk::Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
                .with_component(camera)
                .with_rotation(Quaternion::from_axis_angle(Vec3::Y, std::f32::consts::PI))
                .build(),
        );

        // The ground needs an explicit material: the projection has no
        // default-material fallback, so a mesh without a material handle is
        // skipped (and logged). A matte grey, no texture map.
        let ground_mat = khora_sdk::prelude::materials::StandardMaterial {
            base_color: khora_sdk::prelude::math::LinearRgba::new(0.5, 0.5, 0.5, 1.0),
            roughness: 0.9,
            ..Default::default()
        };
        let ground_handle = world.add_material(ground_mat);
        khora_sdk::spawn_plane(world, 20.0, 0.0)
            .with_component(ground_handle)
            .build();

        let sun_rotation = Quaternion::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2 * 0.8);
        let mut sun_light = khora_sdk::prelude::ecs::Light::directional();
        if let khora_sdk::prelude::ecs::LightType::Directional(ref mut d) = sun_light.light_type {
            d.intensity = 2.5;
            d.shadow_enabled = true;
            d.shadow_bias = 0.005;
            d.shadow_normal_bias = 0.02;
        }

        khora_sdk::Vessel::at(world, Vec3::new(0.0, 20.0, 5.0))
            .with_component(sun_light)
            .with_rotation(sun_rotation)
            .build();

        // Register a procedural checkerboard texture in the shared AssetStore
        // (CpuTexture sub-store); the material projection uploads it to the GPU
        // and every lit lane samples it as the albedo map.
        let checker_uuid = register_checker_texture(runtime);

        // Procedural HDR environment, projected onto the IBL environment cube
        // by the bake. `register_agents` points `EnvironmentMap` at this id.
        register_environment_map(runtime);

        let positions = [
            Vec3::new(0.0, 0.5, -5.0),
            Vec3::new(-3.0, 0.5, -8.0),
            Vec3::new(3.0, 0.5, -6.0),
            Vec3::new(-1.5, 0.5, -4.0),
            Vec3::new(2.0, 0.5, -10.0),
        ];
        let colors = [
            khora_sdk::prelude::math::LinearRgba::RED,
            khora_sdk::prelude::math::LinearRgba::GREEN,
            khora_sdk::prelude::math::LinearRgba::BLUE,
            khora_sdk::prelude::math::LinearRgba::YELLOW,
            khora_sdk::prelude::math::LinearRgba::CYAN,
        ];

        let mut point_light = khora_sdk::prelude::ecs::Light::point();
        if let khora_sdk::prelude::ecs::LightType::Point(ref mut p) = point_light.light_type {
            // `intensity` is a direct linear multiplier on the light color in
            // this shading model (not a physical luminous power), so it pairs
            // with the windowed `(1 - (d/range)^2)^2` attenuation. A few units
            // is bright; large values saturate every nearby surface to white
            // after the Reinhard tonemap.
            p.intensity = 5.0;
            p.color = khora_sdk::prelude::math::LinearRgba::new(0.8, 0.9, 1.0, 1.0);
            p.range = 15.0;
        }
        world.spawn((
            khora_sdk::prelude::ecs::Transform::from_translation(Vec3::new(0.0, 1.0, -2.0)),
            khora_sdk::prelude::ecs::GlobalTransform::default(),
            point_light,
        ));

        for (i, pos) in positions.iter().enumerate() {
            let mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color: colors[i],
                base_color_texture: Some(checker_uuid),
                roughness: 0.2,
                ..Default::default()
            };
            let mat_handle = world.add_material(*Box::new(mat));

            khora_sdk::spawn_sphere(world, 0.75, 32, 16)
                .at_position(*pos)
                .with_component(mat_handle)
                .build();
        }

        // Chrome ball — a smooth metal sphere mirrors the environment almost
        // directly, so it shows what the IBL specular chain is actually
        // sampling (sun, light panels, sky/ground split) rather than the soft
        // sheen a rough dielectric gives.
        let chrome = khora_sdk::prelude::materials::StandardMaterial {
            base_color: khora_sdk::prelude::math::LinearRgba::rgb(0.95, 0.96, 0.98),
            metallic: 1.0,
            roughness: 0.05,
            ..Default::default()
        };
        let chrome_handle = world.add_material(*Box::new(chrome));
        khora_sdk::spawn_sphere(world, 0.9, 48, 24)
            .at_position(Vec3::new(0.0, 0.9, -2.6))
            .with_component(chrome_handle)
            .build();

        // A sphere driven by Ergon rather than by Rust — the first entity in
        // this repository whose motion comes from a script.
        //
        // `Script` names the module and the behaviour; `with_field` overrides a
        // default the script declared, which is exactly what the inspector
        // edits and what a scene file keeps. The behaviour itself is in
        // `assets/scripts/hover.erg`, recompiled from source at startup.
        let scripted = khora_sdk::prelude::materials::StandardMaterial {
            base_color: khora_sdk::prelude::math::LinearRgba::rgb(0.9, 0.45, 0.15),
            roughness: 0.35,
            ..Default::default()
        };
        let scripted_handle = world.add_material(*Box::new(scripted));
        khora_sdk::spawn_sphere(world, 0.45, 32, 16)
            .at_position(Vec3::new(-2.2, 0.8, -1.4))
            .with_component(scripted_handle)
            .with_component(
                khora_sdk::prelude::ecs::Script::new("hover.erg", "Hover")
                    .with_field("speed", khora_sdk::prelude::ecs::ScriptValue::Float(0.9)),
            )
            .build();

        // Two tinted glass spheres, one behind the other, to show alpha
        // blending: `AlphaMode::Blend` puts them in the depth-sorted
        // transparent batch, so the far one shows through the near one and the
        // scene behind shows through both. Overlapping them is the actual test
        // — a single transparent object would look right even unsorted.
        for (offset, tint) in [
            (
                0.0_f32,
                khora_sdk::prelude::math::LinearRgba::new(0.25, 0.85, 0.55, 0.35),
            ),
            (
                1.1,
                khora_sdk::prelude::math::LinearRgba::new(0.95, 0.55, 0.25, 0.35),
            ),
        ] {
            let glass = khora_sdk::prelude::materials::StandardMaterial {
                base_color: tint,
                alpha_mode: khora_sdk::prelude::materials::AlphaMode::Blend,
                // Glass is smooth and double-sided: without back faces the
                // sphere would look hollow once you can see into it.
                roughness: 0.1,
                double_sided: true,
                ..Default::default()
            };
            let glass_handle = world.add_material(*Box::new(glass));
            khora_sdk::spawn_sphere(world, 0.8, 32, 16)
                .at_position(Vec3::new(-2.2 + offset, 0.8, -1.4 - offset))
                .with_component(glass_handle)
                .build();
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        self.frame_count += 1;

        self.controller.process_input(inputs);

        // Real wall-clock delta from the engine's Time resource — gameplay
        // movement is variable-rate, so it uses `delta_seconds` (not the fixed
        // sim step). Falls back to a 60 Hz step if the resource is unavailable.
        let dt = self
            .time
            .as_ref()
            .and_then(|t| t.read().ok().map(|t| t.delta_seconds))
            .unwrap_or(1.0 / 60.0);

        if let Some(player) = self.player {
            if let Some(transform) = world.get_transform_mut(player) {
                if let Some(map_arc) = &self.input_map {
                    if let Ok(map) = map_arc.lock() {
                        self.controller.update(transform, dt, &map);
                    }
                }
            }
            world.sync_global_transform(player);
        }

        if self.frame_count.is_multiple_of(300) {
            let entity_count = world.iter_entities().count();
            let mouse = if self.controller.mouse_captured {
                "captured"
            } else {
                "free"
            };
            log::info!(
                "SandboxGame: Frame {}, {} entities, mouse: {}",
                self.frame_count,
                entity_count,
                mouse
            );
        }
    }
}

impl AgentProvider for SandboxGame {
    fn register_agents(&self, _dcc: &DccService, runtime: &mut Runtime) {
        // No custom agents. This is, however, the engine's sanctioned runtime
        // *mutation* point (`setup` only gets `&Runtime`), so the scene's
        // environment selection is inserted here — before the first tick, and
        // therefore before the one-time IBL bake reads it.
        let env_uuid = khora_sdk::khora_core::asset::AssetUUID::new_v5(ENV_ASSET_NAME);
        runtime
            .resources
            .insert(khora_sdk::EnvironmentMap::from_asset(env_uuid));
    }
}

impl PhaseProvider for SandboxGame {
    fn custom_phases(&self) -> Vec<khora_sdk::ExecutionPhase> {
        Vec::new()
    }

    fn removed_phases(&self) -> Vec<khora_sdk::ExecutionPhase> {
        Vec::new()
    }
}
