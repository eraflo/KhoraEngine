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

//! What colour `SimpleUnlitLane` draws each mesh in: the base colour of the
//! CPU material stored under the mesh's material uuid, white without one.

use khora_core::asset::{AssetHandle, AssetUUID, Material, StandardMaterial};
use khora_core::math::LinearRgba;
use khora_core::sync::RwLockExt;
use khora_data::AssetStore;

use super::fixtures::{Renderer, Scene, Surface, Which};

/// The label of the buffers the unlit lane writes its material uniforms into.
const MATERIAL_RING: &str = "SimpleUnlit Material Ring";

/// The asset store the scene's lanes read from.
fn store(scene: &Scene) -> AssetStore {
    scene
        .runtime
        .resources
        .get::<AssetStore>()
        .expect("the scene's runtime holds its asset store")
        .clone()
}

/// The base colour at the head of each material uniform the lane wrote.
fn base_colours_written(renderer: &Renderer) -> Vec<LinearRgba> {
    renderer
        .device
        .written_into(MATERIAL_RING)
        .iter()
        .map(|bytes| {
            let channel = |at: usize| {
                let mut word = [0u8; 4];
                word.copy_from_slice(&bytes[at * 4..at * 4 + 4]);
                f32::from_le_bytes(word)
            };
            LinearRgba::new(channel(0), channel(1), channel(2), channel(3))
        })
        .collect()
}

/// **The unlit lane draws each mesh in the base colour of the CPU material
/// stored under its material uuid**, and in white when the mesh names no
/// material or one with no CPU copy (its GPU copy alone is resident).
#[test]
fn the_unlit_lane_draws_the_base_colour_its_material_uuid_names() {
    let red = LinearRgba::new(1.0, 0.0, 0.0, 1.0);
    let mut renderer = Renderer::new(Which::Unlit);
    let mut scene = Scene::new()
        .with_mesh(Surface::OPAQUE, 2.0)
        .with_mesh(Surface::OPAQUE, 3.0)
        .with_mesh(Surface::OPAQUE, 4.0);
    let material = AssetUUID::new();
    let red_material: Box<dyn Material> = Box::new(StandardMaterial {
        base_color: red,
        ..Default::default()
    });
    store(&scene)
        .store::<Box<dyn Material>>()
        .write_recovered()
        .insert(material, AssetHandle::new(red_material));
    scene.render_world.meshes[0].material = Some(material);
    scene.render_world.meshes[1].material = None;

    renderer.record(&scene);

    assert_eq!(
        base_colours_written(&renderer),
        [red, LinearRgba::WHITE, LinearRgba::WHITE],
        "the material's colour, then white for no material and for a GPU-only one"
    );
}
