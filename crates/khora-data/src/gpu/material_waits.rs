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

//! [`MaterialWaits`]: the textures each material names, the copies of them
//! its GPU copy was built from, and the materials waiting for theirs.

use std::collections::{HashMap, HashSet};

use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::renderer::api::resource::CpuTexture;

use crate::assets::Assets;

/// The material residency's map from textures to the materials naming them.
///
/// A texture that arrives wakes the materials naming it, and only those: a
/// waiting one is made, a resident one made again if a texture it was built
/// from differs from the store's copy, or is gone. No material is scanned.
#[derive(Default)]
pub(crate) struct MaterialWaits {
    /// The materials whose textures are not all decoded.
    waiting: HashSet<AssetUUID>,
    /// Each texture's materials.
    by_texture: HashMap<AssetUUID, HashSet<AssetUUID>>,
    /// Each material's textures, as last linked.
    textures_of: HashMap<AssetUUID, Vec<AssetUUID>>,
    /// The texture copies each material's GPU copy was built from.
    built_from: HashMap<AssetUUID, Vec<(AssetUUID, AssetHandle<CpuTexture>)>>,
}

impl MaterialWaits {
    /// Records that `material` names `textures`, in place of what it named before.
    pub(crate) fn link(&mut self, material: AssetUUID, textures: Vec<AssetUUID>) {
        if self.textures_of.get(&material) == Some(&textures) {
            return;
        }
        self.unlink(&material);
        for texture in &textures {
            self.by_texture
                .entry(*texture)
                .or_default()
                .insert(material);
        }
        if !textures.is_empty() {
            self.textures_of.insert(material, textures);
        }
    }

    /// Forgets `material`: the textures it named, the copies it was built from, its wait.
    pub(crate) fn forget(&mut self, material: &AssetUUID) {
        self.unlink(material);
        self.waiting.remove(material);
        self.built_from.remove(material);
    }

    /// Notes that `material` waits for its textures, and was built from none.
    pub(crate) fn wait(&mut self, material: AssetUUID) {
        self.built_from.remove(&material);
        self.waiting.insert(material);
    }

    /// Notes that the GPU copy of `material` was built from the copies `textures` holds now.
    pub(crate) fn built(&mut self, material: AssetUUID, textures: &Assets<CpuTexture>) {
        let copies: Vec<(AssetUUID, AssetHandle<CpuTexture>)> = self
            .textures_of
            .get(&material)
            .into_iter()
            .flatten()
            .filter_map(|texture| textures.get(texture).map(|copy| (*texture, copy.clone())))
            .collect();
        if copies.is_empty() {
            self.built_from.remove(&material);
        } else {
            self.built_from.insert(material, copies);
        }
    }

    /// Returns whether a texture `material` was built from is another copy in `textures`, or gone.
    pub(crate) fn textures_changed(
        &self,
        material: &AssetUUID,
        textures: &Assets<CpuTexture>,
    ) -> bool {
        self.built_from
            .get(material)
            .into_iter()
            .flatten()
            .any(|(texture, copy)| textures.get(texture) != Some(copy))
    }

    /// Notes that `material` waits no more.
    pub(crate) fn stop_waiting(&mut self, material: &AssetUUID) {
        self.waiting.remove(material);
    }

    /// Returns whether `material` waits for its textures.
    pub(crate) fn is_waiting(&self, material: &AssetUUID) -> bool {
        self.waiting.contains(material)
    }

    /// Returns the materials naming `texture`.
    pub(crate) fn naming(&self, texture: &AssetUUID) -> impl Iterator<Item = AssetUUID> + '_ {
        self.by_texture.get(texture).into_iter().flatten().copied()
    }

    /// Returns every material naming a texture.
    pub(crate) fn every_linked(&self) -> impl Iterator<Item = AssetUUID> + '_ {
        self.textures_of.keys().copied()
    }

    /// Lets go of the texture copies `material` was built from; its links stay.
    pub(crate) fn release_built_from(&mut self, material: &AssetUUID) {
        self.built_from.remove(material);
    }

    /// Returns every material whose texture copies are held.
    pub(crate) fn every_built(&self) -> impl Iterator<Item = AssetUUID> + '_ {
        self.built_from.keys().copied()
    }

    /// Returns whether the texture copies `material` was built from are held.
    #[cfg(test)]
    pub(crate) fn holds_built_from(&self, material: &AssetUUID) -> bool {
        self.built_from.contains_key(material)
    }

    /// Returns every material this map remembers anything of, some more than once.
    pub(crate) fn every_known(&self) -> impl Iterator<Item = AssetUUID> + '_ {
        self.waiting
            .iter()
            .chain(self.textures_of.keys())
            .chain(self.built_from.keys())
            .copied()
    }

    /// Removes the links of `material` to the textures it named.
    fn unlink(&mut self, material: &AssetUUID) {
        let Some(textures) = self.textures_of.remove(material) else {
            return;
        };
        for texture in textures {
            if let Some(materials) = self.by_texture.get_mut(&texture) {
                materials.remove(material);
                if materials.is_empty() {
                    self.by_texture.remove(&texture);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::renderer::api::resource::CpuTexture;

    use crate::assets::Assets;

    use super::MaterialWaits;

    fn named(waits: &MaterialWaits, texture: &AssetUUID) -> Vec<AssetUUID> {
        waits.naming(texture).collect()
    }

    #[test]
    fn a_texture_names_the_materials_linked_to_it() {
        let paint = AssetUUID::new_v5("textures/paint.png");
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let door = AssetUUID::new_v5("materials/door.kmat");
        let mut waits = MaterialWaits::default();
        waits.link(wall, vec![paint]);
        waits.link(door, vec![paint]);

        let materials: std::collections::HashSet<AssetUUID> = waits.naming(&paint).collect();
        assert_eq!(materials, [wall, door].into_iter().collect());
        assert_eq!(waits.every_linked().count(), 2);
    }

    #[test]
    fn linking_again_replaces_the_textures_a_material_named() {
        let old = AssetUUID::new_v5("textures/old.png");
        let new = AssetUUID::new_v5("textures/new.png");
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let mut waits = MaterialWaits::default();
        waits.link(wall, vec![old]);
        waits.link(wall, vec![new]);

        assert!(named(&waits, &old).is_empty(), "no longer named");
        assert_eq!(named(&waits, &new), vec![wall]);
    }

    #[test]
    fn a_forgotten_material_neither_waits_nor_is_named() {
        let paint = AssetUUID::new_v5("textures/paint.png");
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let mut waits = MaterialWaits::default();
        waits.link(wall, vec![paint]);
        waits.wait(wall);
        assert!(waits.is_waiting(&wall));

        waits.forget(&wall);
        assert!(!waits.is_waiting(&wall));
        assert!(named(&waits, &paint).is_empty());
        assert_eq!(waits.every_linked().count(), 0);
    }

    /// A decoded 1×1 RGBA texture.
    fn one_pixel() -> AssetHandle<CpuTexture> {
        use khora_core::renderer::api::resource::{
            SampleCount, TextureDimension, TextureFormat, TextureUsage,
        };
        AssetHandle::new(CpuTexture {
            pixels: vec![255; 4],
            size: khora_core::math::Extent3D {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }

    #[test]
    fn a_material_built_from_a_texture_copy_changes_only_when_that_copy_does() {
        let paint = AssetUUID::new_v5("textures/paint.png");
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let mut textures = Assets::new();
        textures.insert(paint, one_pixel());
        let mut waits = MaterialWaits::default();
        waits.link(wall, vec![paint]);
        waits.built(wall, &textures);
        assert!(!waits.textures_changed(&wall, &textures), "the same copy");

        textures.insert(paint, one_pixel());
        assert!(waits.textures_changed(&wall, &textures), "another copy");
        waits.built(wall, &textures);
        textures.remove(&paint);
        assert!(waits.textures_changed(&wall, &textures), "gone");
    }

    #[test]
    fn releasing_the_copies_a_material_was_built_from_keeps_its_links() {
        let paint = AssetUUID::new_v5("textures/paint.png");
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let mut textures = Assets::new();
        textures.insert(paint, one_pixel());
        let mut waits = MaterialWaits::default();
        waits.link(wall, vec![paint]);
        waits.built(wall, &textures);
        assert_eq!(waits.every_built().collect::<Vec<_>>(), vec![wall]);

        waits.release_built_from(&wall);
        assert_eq!(waits.every_built().count(), 0, "no copy held");
        assert_eq!(
            waits.naming(&paint).collect::<Vec<_>>(),
            vec![wall],
            "the link stays"
        );
    }

    #[test]
    fn every_material_remembered_is_known_until_forgotten() {
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let mut waits = MaterialWaits::default();
        waits.wait(wall);
        assert_eq!(waits.every_known().collect::<Vec<_>>(), vec![wall]);
        waits.forget(&wall);
        assert_eq!(waits.every_known().count(), 0);
    }

    #[test]
    fn a_material_stops_waiting() {
        let wall = AssetUUID::new_v5("materials/wall.kmat");
        let mut waits = MaterialWaits::default();
        waits.wait(wall);
        waits.stop_waiting(&wall);
        assert!(!waits.is_waiting(&wall));
    }
}
