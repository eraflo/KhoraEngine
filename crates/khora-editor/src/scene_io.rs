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

//! Scene serialization and project-asset scanning helpers.

use crate::util::{bytemuck_transform, unbytemuck_transform};
use khora_agents::serialization_service::SerializationService;
use khora_core::scene::{SceneFile, SerializationGoal};
use khora_core::ui::editor::*;
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::*;
use khora_sdk::GameWorld;

/// Serializes all entity transforms into a binary snapshot for play-mode restore.
pub fn snapshot_scene(world: &GameWorld) -> Vec<u8> {
    let entities: Vec<EntityId> = world.iter_entities().collect();
    let mut data: Vec<u8> = Vec::new();

    let count = entities.len() as u32;
    data.extend_from_slice(&count.to_le_bytes());

    for &entity in &entities {
        data.extend_from_slice(&entity.index.to_le_bytes());
        data.extend_from_slice(&entity.generation.to_le_bytes());

        if let Some(t) = world.get_component::<Transform>(entity) {
            data.push(1);
            data.extend_from_slice(&bytemuck_transform(t));
        } else {
            data.push(0);
        }
    }

    data
}

/// Restores entity transforms from a binary snapshot.
pub fn restore_scene(world: &mut GameWorld, snapshot: &[u8]) {
    if snapshot.len() < 4 {
        return;
    }

    let count = u32::from_le_bytes([snapshot[0], snapshot[1], snapshot[2], snapshot[3]]) as usize;
    let mut offset = 4;

    for _ in 0..count {
        if offset + 8 > snapshot.len() {
            break;
        }

        let index = u32::from_le_bytes([
            snapshot[offset],
            snapshot[offset + 1],
            snapshot[offset + 2],
            snapshot[offset + 3],
        ]);
        let generation = u32::from_le_bytes([
            snapshot[offset + 4],
            snapshot[offset + 5],
            snapshot[offset + 6],
            snapshot[offset + 7],
        ]);
        offset += 8;

        let entity = EntityId { index, generation };

        if offset >= snapshot.len() {
            break;
        }

        let has_transform = snapshot[offset];
        offset += 1;

        if has_transform == 1 {
            if offset + 40 > snapshot.len() {
                break;
            }

            let transform = unbytemuck_transform(&snapshot[offset..offset + 40]);
            offset += 40;

            if let Some(existing) = world.get_component_mut::<Transform>(entity) {
                *existing = transform;
            }
        }
    }
}

/// Serializes the current scene to a KHORASCN file at the given path.
pub fn save_scene_to(world: &GameWorld, path: &str) {
    let agent = SerializationService::new();
    match agent.save_world(world.inner_world(), SerializationGoal::EditorInterchange) {
        Ok(scene_file) => {
            let bytes = scene_file.to_bytes();
            match std::fs::write(path, &bytes) {
                Ok(()) => log::info!("Scene saved to '{}' ({} bytes)", path, bytes.len()),
                Err(e) => log::error!("Failed to write scene file '{}': {}", path, e),
            }
        }
        Err(e) => log::error!("Failed to serialize scene: {:?}", e),
    }
}

/// Loads a KHORASCN file from disk and replaces the current scene.
pub fn load_scene_from(world: &mut GameWorld, path: &str) {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("Failed to read scene file '{}': {}", path, e);
            return;
        }
    };

    let scene_file = match SceneFile::from_bytes(&bytes) {
        Ok(file) => file,
        Err(e) => {
            log::error!("Invalid scene file '{}': {:?}", path, e);
            return;
        }
    };

    let all_entities: Vec<_> = world.iter_entities().collect();
    for entity in all_entities {
        world.despawn(entity);
    }

    let agent = SerializationService::new();
    match agent.load_world(&scene_file, world.inner_world_mut()) {
        Ok(()) => log::info!("Scene loaded from '{}' ({} bytes)", path, bytes.len()),
        Err(e) => log::error!("Failed to deserialize scene '{}': {:?}", path, e),
    }
}

/// Recursively scans a project folder and returns recognized asset entries.
pub fn scan_project_folder(root: &std::path::Path) -> Vec<AssetEntry> {
    let mut entries = Vec::new();
    scan_dir(root, &mut entries);
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn scan_dir(dir: &std::path::Path, entries: &mut Vec<AssetEntry>) {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => return,
    };

    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, entries);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let asset_type = match ext.to_lowercase().as_str() {
                "gltf" | "glb" | "obj" | "fbx" => "Mesh",
                "png" | "jpg" | "jpeg" | "tga" | "bmp" | "hdr" => "Texture",
                "wav" | "ogg" | "mp3" | "flac" => "Audio",
                "wgsl" | "hlsl" | "glsl" => "Shader",
                "ttf" | "otf" => "Font",
                "scene" | "kscene" => "Scene",
                "mat" | "kmat" => "Material",
                _ => continue,
            };

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            entries.push(AssetEntry {
                name,
                asset_type: asset_type.to_owned(),
                source_path: path.to_string_lossy().to_string(),
            });
        }
    }
}
