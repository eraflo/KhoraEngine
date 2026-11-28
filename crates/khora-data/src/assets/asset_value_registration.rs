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

//! [`AssetValueRegistration`], the Rust type an asset kind's values have, and
//! the bindings of the kinds whose values core defines.

use std::any::TypeId;

use khora_core::asset::font::Font;
use khora_core::asset::{Asset, AssetKind, Material};
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;

/// Binds an asset kind to the Rust type of its values.
///
/// One per kind whose values a reference names; a kind with no value type (a
/// scene, a script) has none.
pub struct AssetValueRegistration {
    /// The kind whose values have this type.
    pub kind: AssetKind,
    /// Returns the `TypeId` of the kind's values.
    pub value: fn() -> TypeId,
}

impl AssetValueRegistration {
    /// Returns the binding of `A`'s kind to `A`.
    pub const fn of<A: Asset>() -> Self {
        Self {
            kind: A::KIND,
            value: TypeId::of::<A>,
        }
    }
}

inventory::collect!(AssetValueRegistration);

inventory::submit!(AssetValueRegistration::of::<Mesh>());
inventory::submit!(AssetValueRegistration::of::<Box<dyn Material>>());
inventory::submit!(AssetValueRegistration::of::<CpuTexture>());
inventory::submit!(AssetValueRegistration::of::<Font>());

#[cfg(test)]
mod tests {
    use khora_core::asset::kinds;

    use super::*;

    /// A binding names the value type's own kind and type.
    #[test]
    fn a_binding_names_its_value_type_and_kind() {
        let binding = AssetValueRegistration::of::<Mesh>();
        assert_eq!(binding.kind, kinds::MESH);
        assert_eq!((binding.value)(), TypeId::of::<Mesh>());
    }
}
