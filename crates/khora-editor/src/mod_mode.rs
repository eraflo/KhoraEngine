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

//! Editor mode management.
//!
//! Handles synchronization between the editor's PlayMode and the engine's
//! EngineMode, including world snapshotting and restoration.

use std::sync::{Arc, RwLock};

use khora_sdk::{DccService, EngineContext, EngineMode, GameWorld, SerializationGoal, SerializationService, TelemetryEvent};
use khora_sdk::EcsWorld;

/// Switches the engine between modes.
///
/// - **Custom → Playing**: Saves the current world state (snapshot).
/// - **Playing → Custom**: Restores the saved world state from snapshot.
pub fn set_mode(
    mode: EngineMode,
    context: &Arc<RwLock<EngineContext>>,
    dcc: &Option<DccService>,
    world: &mut GameWorld,
    snapshot: &mut Option<Vec<u8>>,
) {
    let current_mode = {
        let ctx = context.read().unwrap();
        ctx.mode.clone()
    };

    if current_mode == mode {
        return;
    }

    // Custom mode → Playing: snapshot the world
    if matches!(&current_mode, EngineMode::Custom(_)) && mode == EngineMode::Playing {
        log::info!("Engine: Entering Play mode");
        let service = SerializationService::new();
        match service.save_world(
            world.inner_world(),
            SerializationGoal::FastestLoad,
        ) {
            Ok(scene_file) => {
                *snapshot = Some(scene_file.to_bytes());
                log::info!(
                    "Engine: World snapshot saved ({} bytes)",
                    scene_file.payload.len()
                );
            }
            Err(e) => {
                log::error!("Engine: Failed to snapshot world: {:?}", e);
            }
        }
    }

    // Playing → Custom mode: restore the world from snapshot
    if matches!(&mode, EngineMode::Custom(_)) && current_mode == EngineMode::Playing {
        log::info!("Engine: Exiting Play mode");
        if let (Some(snapshot_bytes), Some(gw)) = (&*snapshot, Some(world)) {
            let scene_file = match khora_sdk::SceneFile::from_bytes(snapshot_bytes) {
                Ok(f) => f,
                Err(e) => {
                    log::error!("Engine: Failed to parse world snapshot: {:?}", e);
                    *snapshot = None;
                    return;
                }
            };
            let service = SerializationService::new();
            let mut new_world = EcsWorld::new();
            match service.load_world(&scene_file, &mut new_world) {
                Ok(()) => {
                    *gw = GameWorld::from_world(new_world);
                    log::info!("Engine: World restored from snapshot");
                }
                Err(e) => {
                    log::error!("Engine: Failed to restore world: {:?}", e);
                }
            }
            *snapshot = None;
        }
    }

    let mode_name = mode.name().to_string();

    // Update the context mode
    {
        let mut ctx = context.write().unwrap();
        ctx.mode = mode;
    }

    // Notify the DCC
    if let Some(dcc) = dcc {
        let _ = dcc
            .event_sender()
            .send(TelemetryEvent::PhaseChange(mode_name));
    }
}

/// Determines the target engine mode from the editor's play mode.
pub fn play_mode_to_engine_mode(play_mode: khora_sdk::PlayMode) -> EngineMode {
    match play_mode {
        khora_sdk::PlayMode::Editing => EngineMode::Custom("editor".to_string()),
        khora_sdk::PlayMode::Playing | khora_sdk::PlayMode::Paused => EngineMode::Playing,
    }
}

/// Returns the current engine mode from the context.
pub fn get_current_mode(
    context: &Arc<RwLock<EngineContext>>,
) -> EngineMode {
    context.read().unwrap().mode.clone()
}
