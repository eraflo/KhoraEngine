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

use anyhow::Result;
use khora_agents::asset_agent::agent::AssetAgent;
use khora_core::asset::{Asset, AssetMetadata, AssetSource, AssetUUID};
use khora_lanes::asset_lane::AssetLoaderLane;
use std::{collections::HashMap, error::Error, fs::File};
use tempfile::tempdir;

// --- Test Setup: Dummy Asset and Loader (reste identique) ---
#[derive(Debug, PartialEq)]
struct TestTexture {
    id: u32,
}
impl Asset for TestTexture {}

struct TestTextureLoader;
impl AssetLoaderLane<TestTexture> for TestTextureLoader {
    fn load(&self, bytes: &[u8]) -> Result<TestTexture, Box<dyn Error + Send + Sync>> {
        let id = u32::from_le_bytes(bytes.try_into().unwrap());
        Ok(TestTexture { id })
    }
}
// ---

#[test]
fn test_load_asset_from_pack() -> Result<()> {
    // --- 1. Setup: Create REAL temporary packfiles on disk ---
    let dir = tempdir()?;
    let index_path = dir.path().join("index.bin");
    let data_path = dir.path().join("data.pack");

    let texture_uuid = AssetUUID::new_v5("test/texture.png");
    let texture_data = 1234u32.to_le_bytes();

    let mut variants = HashMap::new();
    variants.insert(
        "default".to_string(),
        AssetSource::Packed {
            offset: 0,
            size: texture_data.len() as u64,
        },
    );
    let metadata = AssetMetadata {
        uuid: texture_uuid,
        source_path: "test/texture.png".into(),
        asset_type_name: "texture".to_string(),
        dependencies: vec![],
        variants,
        tags: vec![],
    };

    let metadata_vec = vec![metadata];
    let config = bincode::config::standard();
    let index_bytes = bincode::serde::encode_to_vec(&metadata_vec, config)?;
    let data_bytes = texture_data.to_vec();

    // Write the temporary files to disk.
    std::fs::write(&index_path, &index_bytes)?;
    std::fs::write(&data_path, &data_bytes)?;

    // --- 2. Initialize the AssetAgent with REAL files ---
    let data_file = File::open(&data_path)?;
    let mut asset_agent = AssetAgent::new(&index_bytes, data_file)?;

    // --- 3. Register the loader ---
    asset_agent.register_loader("texture", TestTextureLoader);

    // --- 4. Load the asset ---
    let texture_handle = asset_agent.load::<TestTexture>(&texture_uuid)?;

    // --- 5. Assert: Verify the result ---
    assert_eq!(texture_handle.id, 1234);

    println!("Integration test passed: Asset loaded and decoded correctly from temp files.");
    Ok(())
}

#[test]
fn test_load_texture_from_pack() -> Result<()> {
    use image::ImageEncoder;
    use khora_core::renderer::api::CpuTexture;
    use khora_lanes::asset_lane::TextureLoaderLane;

    // --- 1. Setup: Create temporary packfiles with a real PNG ---
    let dir = tempdir()?;
    let index_path = dir.path().join("index.bin");
    let data_path = dir.path().join("data.pack");

    // Create a simple 2x2 PNG image in memory
    let width = 2;
    let height = 2;
    let mut image_data = Vec::new();
    // Pixels: (0,0): red, (1,0): green, (0,1): blue, (1,1): white
    image_data.extend_from_slice(&[255, 0, 0, 255]); // Red
    image_data.extend_from_slice(&[0, 255, 0, 255]); // Green
    image_data.extend_from_slice(&[0, 0, 255, 255]); // Blue
    image_data.extend_from_slice(&[255, 255, 255, 255]); // White

    // Encode as PNG
    let mut png_data = Vec::new();
    {
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        encoder.write_image(&image_data, width, height, image::ExtendedColorType::Rgba8)?;
    }

    let texture_uuid = AssetUUID::new_v5("test/texture.png");
    let mut variants = HashMap::new();
    variants.insert(
        "default".to_string(),
        AssetSource::Packed {
            offset: 0,
            size: png_data.len() as u64,
        },
    );
    let metadata = AssetMetadata {
        uuid: texture_uuid,
        source_path: "test/texture.png".into(),
        asset_type_name: "texture".to_string(),
        dependencies: vec![],
        variants,
        tags: vec![],
    };

    let metadata_vec = vec![metadata];
    let config = bincode::config::standard();
    let index_bytes = bincode::serde::encode_to_vec(&metadata_vec, config)?;

    // Write the temporary files to disk
    std::fs::write(&index_path, &index_bytes)?;
    std::fs::write(&data_path, &png_data)?;

    // --- 2. Initialize the AssetAgent with REAL files ---
    let data_file = File::open(&data_path)?;
    let mut asset_agent = AssetAgent::new(&index_bytes, data_file)?;

    // --- 3. Register the texture loader ---
    asset_agent.register_loader("texture", TextureLoaderLane);

    // --- 4. Load the texture ---
    let texture_handle = asset_agent.load::<CpuTexture>(&texture_uuid)?;

    // --- 5. Assert: Verify the texture was loaded correctly ---
    assert_eq!(texture_handle.pixels.len(), 16); // 2x2 RGBA = 16 bytes
    assert_eq!(texture_handle.size.width, 2);
    assert_eq!(texture_handle.size.height, 2);
    assert_eq!(
        texture_handle.format,
        khora_core::renderer::api::TextureFormat::Rgba8UnormSrgb
    );

    println!("Texture loading test passed: PNG texture loaded and decoded correctly");
    Ok(())
}
