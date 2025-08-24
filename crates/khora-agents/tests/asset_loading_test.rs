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
use khora_lanes::asset_lane::AssetLoader;
use std::{collections::HashMap, error::Error, fs::File};
use tempfile::tempdir;

// --- Test Setup: Dummy Asset and Loader (reste identique) ---
#[derive(Debug, PartialEq)]
struct TestTexture {
    id: u32,
}
impl Asset for TestTexture {}

struct TestTextureLoader;
impl AssetLoader<TestTexture> for TestTextureLoader {
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
