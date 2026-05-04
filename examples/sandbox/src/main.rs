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

//! Sandbox example demonstrating high-level game logic with Khora Engine.
//!
//! This example shows how to build a game using only the SDK's public API.
//! No low-level rendering details - pure game logic.
//!
//! Controls:
//! - Right mouse button + drag: Look around
//! - W/A/S/D: Move forward/left/backward/right
//! - Space: Move up
//! - Shift: Move down

use anyhow::Result;
use khora_sdk::prelude::math::{Quaternion, Vec3};
use khora_sdk::prelude::*;
use khora_sdk::run_winit;
use khora_sdk::winit_adapters::WinitWindowProvider;
use khora_sdk::{
    AgentProvider, DccService, EngineApp, GameWorld, InputEvent, PhaseProvider, RenderSystem,
    ServiceRegistry, WgpuRenderSystem, WindowConfig,
};
use std::sync::{Arc, Mutex};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

/// Simple camera controller for the player.
struct PlayerController {
    speed: f32,
    sensitivity: f32,
    yaw: f32,
    pitch: f32,
    move_forward: f32,
    move_right: f32,
    move_up: f32,
    mouse_captured: bool,
    last_mouse: (f32, f32),
    keys_held: std::collections::HashSet<String>,
}

impl PlayerController {
    fn new() -> Self {
        Self {
            speed: 5.0,
            sensitivity: 0.003,
            yaw: std::f32::consts::PI,
            pitch: 0.0,
            move_forward: 0.0,
            move_right: 0.0,
            move_up: 0.0,
            mouse_captured: false,
            last_mouse: (0.0, 0.0),
            keys_held: std::collections::HashSet::new(),
        }
    }

    fn process_input(&mut self, inputs: &[InputEvent]) {
        use khora_sdk::prelude::MouseButton;

        for event in inputs {
            match event {
                InputEvent::MouseButtonPressed { button } if *button == MouseButton::Right => {
                    self.mouse_captured = true;
                }
                InputEvent::MouseButtonReleased { button } if *button == MouseButton::Right => {
                    self.mouse_captured = false;
                }
                InputEvent::MouseMoved { x, y } => {
                    if self.mouse_captured {
                        let dx = x - self.last_mouse.0;
                        let dy = y - self.last_mouse.1;

                        self.yaw -= dx * self.sensitivity;
                        self.pitch -= dy * self.sensitivity;
                        self.pitch = self.pitch.clamp(
                            -std::f32::consts::FRAC_PI_2 + 0.01,
                            std::f32::consts::FRAC_PI_2 - 0.01,
                        );
                    }
                    self.last_mouse = (*x, *y);
                }
                InputEvent::KeyPressed { key_code } => {
                    self.handle_key(key_code, true);
                }
                InputEvent::KeyReleased { key_code } => {
                    self.handle_key(key_code, false);
                }
                _ => {}
            }
        }

        self.recalculate_movement();
    }

    fn handle_key(&mut self, key: &str, pressed: bool) {
        if pressed {
            self.keys_held.insert(key.to_string());
        } else {
            self.keys_held.remove(key);
        }
    }

    fn recalculate_movement(&mut self) {
        self.move_forward = 0.0;
        self.move_right = 0.0;
        self.move_up = 0.0;

        for key in &self.keys_held {
            match key.as_str() {
                "KeyW" => self.move_forward -= 1.0,
                "KeyS" => self.move_forward += 1.0,
                "KeyA" => self.move_right -= 1.0,
                "KeyD" => self.move_right += 1.0,
                "Space" => self.move_up += 1.0,
                "ShiftLeft" | "ShiftRight" => self.move_up -= 1.0,
                _ => {}
            }
        }
    }

    fn update(&self, transform: &mut khora_sdk::prelude::ecs::Transform, delta_time: f32) {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        let forward = Vec3::new(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch);
        let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);

        let velocity = self.speed * delta_time;
        transform.translation = transform.translation
            + forward * (-self.move_forward) * velocity
            + right * self.move_right * velocity
            + Vec3::Y * self.move_up * velocity;

        let yaw_quat = Quaternion::from_axis_angle(Vec3::Y, self.yaw);
        let pitch_quat = Quaternion::from_axis_angle(Vec3::X, self.pitch);
        transform.rotation = yaw_quat * pitch_quat;
    }
}

/// A simple game demonstrating the Khora SDK.
struct SandboxGame {
    frame_count: u64,
    player: Option<khora_sdk::prelude::ecs::EntityId>,
    controller: PlayerController,
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
        }
    }

    fn setup(&mut self, world: &mut GameWorld, _services: &ServiceRegistry) {
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

        khora_sdk::spawn_plane(world, 20.0, 0.0).build();

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
            p.intensity = 500.0;
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
                roughness: 0.2,
                ..Default::default()
            };
            let mat_handle = world.add_material(*Box::new(mat));

            khora_sdk::spawn_sphere(world, 0.75, 32, 16)
                .at_position(*pos)
                .with_component(mat_handle)
                .build();
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        self.frame_count += 1;

        self.controller.process_input(inputs);

        if let Some(player) = self.player {
            if let Some(transform) = world.get_transform_mut(player) {
                self.controller.update(transform, 0.016);
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
    fn register_agents(&self, _dcc: &DccService, _services: &mut ServiceRegistry) {
        // Sandbox doesn't register custom agents
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

fn main() -> Result<()> {
    use env_logger::{Builder, Env};

    Builder::from_env(Env::default().default_filter_or("info"))
        // Suppress Epic Games / EOS overlay Vulkan loader JSON-not-found noise.
        // These are harmless OS-level loader warnings, not engine errors.
        .filter_module("wgpu_hal::vulkan::instance", log::LevelFilter::Off)
        .init();

    run_winit::<WinitWindowProvider, SandboxGame>(|window, services, _event_loop| {
        let mut rs = WgpuRenderSystem::new();
        rs.init(window).expect("renderer init failed");
        // Register the graphics device before boxing — required by RenderAgent.
        services.insert(rs.graphics_device());
        let rs: Box<dyn RenderSystem> = Box::new(rs);
        services.insert(Arc::new(Mutex::new(rs)));
    })?;
    Ok(())
}
