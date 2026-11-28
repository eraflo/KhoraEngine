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

//! [`InlineMaterial`], a material a reference carries itself.

use super::Material;
use crate::asset::{AssetUUID, ContentHasher, LocalSource};

/// A material a reference carries itself, named by its content.
///
/// Its uuid is the hash of the material's content
/// ([`Material::hash_content`]): equal materials share one uuid, so one copy
/// in the store and on a device; nothing else can name it.
#[derive(Clone)]
pub struct InlineMaterial {
    material: Box<dyn Material>,
    uuid: AssetUUID,
}

impl InlineMaterial {
    /// Returns `material` as a local source, its uuid derived from its content.
    pub fn new(material: Box<dyn Material>) -> Self {
        let mut hasher = ContentHasher::new();
        material.hash_content(&mut hasher);
        Self {
            uuid: hasher.uuid(),
            material,
        }
    }

    /// Returns the material the source carries.
    pub fn material(&self) -> &dyn Material {
        &*self.material
    }
}

impl LocalSource<Box<dyn Material>> for InlineMaterial {
    const VARIANT: &'static str = "Inline";

    fn uuid(&self) -> AssetUUID {
        self.uuid
    }

    fn build(&self) -> Box<dyn Material> {
        self.material.clone_box()
    }
}

/// Two inline materials are equal when their content is: their uuids are.
impl PartialEq for InlineMaterial {
    fn eq(&self, other: &Self) -> bool {
        self.uuid == other.uuid
    }
}

impl std::fmt::Debug for InlineMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InlineMaterial")
            .field("uuid", &self.uuid)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{StandardMaterial, UnlitMaterial};
    use crate::math::LinearRgba;

    /// Equal content gives one uuid; other content or another type another.
    #[test]
    fn an_inline_material_is_named_by_its_content() {
        let red = || {
            InlineMaterial::new(Box::new(StandardMaterial {
                base_color: LinearRgba::RED,
                ..StandardMaterial::default()
            }))
        };
        assert_eq!(red().uuid(), red().uuid());
        assert_eq!(red(), red());
        assert_ne!(
            red().uuid(),
            InlineMaterial::new(Box::new(StandardMaterial::default())).uuid()
        );
        assert_ne!(
            InlineMaterial::new(Box::new(StandardMaterial::default())).uuid(),
            InlineMaterial::new(Box::new(UnlitMaterial::default())).uuid()
        );
    }

    /// The material built is the one carried.
    #[test]
    fn an_inline_material_builds_what_it_carries() {
        let source = InlineMaterial::new(Box::new(UnlitMaterial {
            base_color: LinearRgba::BLUE,
            ..UnlitMaterial::default()
        }));
        assert_eq!(source.build().base_color(), LinearRgba::BLUE);
        assert_eq!(source.material().base_color(), LinearRgba::BLUE);
    }
}
