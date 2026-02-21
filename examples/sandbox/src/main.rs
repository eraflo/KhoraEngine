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
use khora_sdk::{Application, Engine, EngineContext, GameWorld, InputEvent};

#[global_allocator]
static GLOBAL: SaaTrackingAllocator = SaaTrackingAllocator::new(std::alloc::System);

/// Simple camera controller for the player.
struct PlayerController {
    /// Movement speed in units per second.
    speed: f32,
    /// Mouse sensitivity.
    sensitivity: f32,
    /// Current yaw angle (rotation around Y axis).
    yaw: f32,
    /// Current pitch angle (rotation around X axis).
    pitch: f32,
    /// Current movement input.
    move_forward: f32,
    move_right: f32,
    move_up: f32,
    /// Whether mouse is captured.
    mouse_captured: bool,
    /// Last mouse position.
    last_mouse: (f32, f32),
    /// Track which keys are currently held to prevent release-before-use bugs
    keys_held: std::collections::HashSet<String>,
}

impl PlayerController {
    fn new() -> Self {
        Self {
            speed: 5.0,
            sensitivity: 0.003,
            yaw: std::f32::consts::PI, // Look towards -Z (same as camera)
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

        if !inputs.is_empty() {
            log::debug!("process_input called with {} inputs", inputs.len());
        }

        for event in inputs {
            match event {
                InputEvent::MouseButtonPressed { button } => {
                    log::debug!("Mouse pressed: {:?}", button);
                    if *button == MouseButton::Right {
                        self.mouse_captured = true;
                    }
                }
                InputEvent::MouseButtonReleased { button } => {
                    log::debug!("Mouse released: {:?}", button);
                    if *button == MouseButton::Right {
                        self.mouse_captured = false;
                    }
                }
                InputEvent::MouseMoved { x, y } => {
                    if self.mouse_captured {
                        let dx = x - self.last_mouse.0;
                        let dy = y - self.last_mouse.1;
                        log::trace!("Mouse moved: ({}, {}) delta: ({}, {})", x, y, dx, dy);

                        self.yaw -= dx * self.sensitivity;
                        self.pitch -= dy * self.sensitivity;

                        // Clamp pitch
                        self.pitch = self.pitch.clamp(
                            -std::f32::consts::FRAC_PI_2 + 0.01,
                            std::f32::consts::FRAC_PI_2 - 0.01,
                        );
                    }
                    self.last_mouse = (*x, *y);
                }
                InputEvent::KeyPressed { key_code } => {
                    log::debug!("Key pressed: {}", key_code);
                    self.handle_key(key_code, true);
                }
                InputEvent::KeyReleased { key_code } => {
                    log::debug!("Key released: {}", key_code);
                    self.handle_key(key_code, false);
                }
                _ => {}
            }
        }

        // Recalculate movement after processing all events
        // This ensures keys held during the frame contribute to movement
        self.recalculate_movement();
    }

    fn handle_key(&mut self, key: &str, pressed: bool) {
        if pressed {
            self.keys_held.insert(key.to_string());
        } else {
            self.keys_held.remove(key);
        }
        log::trace!(
            "handle_key: {} pressed={} keys_held={:?}",
            key,
            pressed,
            self.keys_held
        );
    }

    fn recalculate_movement(&mut self) {
        // Reset movement
        self.move_forward = 0.0;
        self.move_right = 0.0;
        self.move_up = 0.0;

        // Recalculate from held keys
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

        log::trace!(
            "recalculate_movement: move_forward={} move_right={} move_up={}",
            self.move_forward,
            self.move_right,
            self.move_up
        );
    }

    fn update(&self, transform: &mut khora_sdk::prelude::ecs::Transform, delta_time: f32) {
        // Calculate directions
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_pitch, cos_pitch) = self.pitch.sin_cos();

        let forward = Vec3::new(sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch);
        let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);

        // Apply movement
        let velocity = self.speed * delta_time;
        transform.translation = transform.translation
            + forward * (-self.move_forward) * velocity
            + right * self.move_right * velocity
            + Vec3::Y * self.move_up * velocity;

        // Update rotation
        let yaw_quat = Quaternion::from_axis_angle(Vec3::Y, self.yaw);
        let pitch_quat = Quaternion::from_axis_angle(Vec3::X, self.pitch);
        transform.rotation = yaw_quat * pitch_quat;
    }
}

/// A simple game demonstrating the Khora SDK.
struct SandboxGame {
    /// Number of frames rendered.
    frame_count: u64,
    /// The player entity (Vessel with camera).
    player: Option<khora_sdk::prelude::ecs::EntityId>,
    /// Player controller.
    controller: PlayerController,
}

impl Application for SandboxGame {
    fn new(_context: EngineContext) -> Self {
        log::info!("SandboxGame: Initializing...");
        Self {
            frame_count: 0,
            player: None,
            controller: PlayerController::new(),
        }
    }

    fn setup(&mut self, world: &mut GameWorld) {
        // Create player - a Vessel with camera
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
        log::info!("SandboxGame: Player spawned with camera");

        // Create floor
        khora_sdk::spawn_plane(world, 20.0, 0.0).build();
        log::info!("SandboxGame: Floor spawned");

        // Create global light (sun) pointing towards -Z and heavily downwards
        let sun_rotation = Quaternion::from_axis_angle(Vec3::X, -std::f32::consts::FRAC_PI_2 * 0.8);
        let mut sun_light = khora_sdk::prelude::ecs::Light::directional();
        if let khora_sdk::prelude::ecs::LightType::Directional(ref mut d) = sun_light.light_type {
            d.intensity = 2.5; // Bump intensity to see diffuse clearly
        }

        khora_sdk::Vessel::at(world, Vec3::new(0.0, 10.0, 5.0))
            .with_component(sun_light)
            .with_rotation(sun_rotation)
            .build();
        log::info!("SandboxGame: Global light spawned");

        // Create cubes
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

        // Let's also add a moving point light to show dynamic gradients
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

        // Create spheres instead of cubes! Spheres have smooth normals, so they will
        // clearly show the beautiful Blinn-Phong shading gradients and specular highlights!
        for (i, pos) in positions.iter().enumerate() {
            let mat = khora_sdk::prelude::materials::StandardMaterial {
                base_color: colors[i],
                roughness: 0.2, // nice glossy highlight
                ..Default::default()
            };
            let mat_handle = world.add_material(*Box::new(mat));

            khora_sdk::spawn_sphere(world, 0.75, 32, 16)
                .at_position(*pos)
                .with_component(mat_handle)
                .build();

            log::info!(
                "SandboxGame: Sphere {} spawned at {:?} with color {:?}",
                i,
                pos,
                colors[i]
            );
        }
    }

    fn update(&mut self, world: &mut GameWorld, inputs: &[InputEvent]) {
        self.frame_count += 1;

        // Process input
        self.controller.process_input(inputs);

        log::trace!(
            "After process_input: move_forward={}, move_right={}",
            self.controller.move_forward,
            self.controller.move_right
        );

        // Update player
        if let Some(player) = self.player {
            if let Some(transform) = world.get_transform_mut(player) {
                let old_pos = transform.translation;
                log::debug!(
                    "Before update: pos={:?}, yaw={}, pitch={}, move_forward={}, move_right={}",
                    old_pos,
                    self.controller.yaw,
                    self.controller.pitch,
                    self.controller.move_forward,
                    self.controller.move_right
                );

                self.controller.update(transform, 0.016);

                let new_pos = transform.translation;
                log::debug!("After update: pos={:?}", new_pos);

                if (old_pos - new_pos).length() > 0.001 {
                    log::debug!("Player moved: {:?} -> {:?}", old_pos, new_pos);
                }
            }
            // Sync to GlobalTransform
            world.sync_global_transform(player);
        }

        // Status log
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

fn main() -> Result<()> {
    use env_logger::{Builder, Env};

    Builder::from_env(Env::default().default_filter_or("info"))
        .filter_module("wgpu_hal", log::LevelFilter::Error)
        .init();

    Engine::run::<SandboxGame>()?;
    Ok(())
}
