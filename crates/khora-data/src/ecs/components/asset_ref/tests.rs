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

//! What an asset reference names, in each of its forms.

use khora_core::asset::{AssetUUID, LocalSource, ProceduralMesh, ProceduralMeshKind};
use khora_core::renderer::api::gpu_scene::Mesh;

use super::AssetRef;

/// Each form names one uuid: the asset's, the local source's own, the made value's.
#[test]
fn a_reference_names_the_uuid_of_its_form() {
    let asset = AssetUUID::new_v5("meshes/rock.gltf");
    let made = AssetUUID::new();
    let sphere = ProceduralMesh::new(ProceduralMeshKind::Sphere, [0.5, 16.0, 16.0, 0.0]);

    assert_eq!(AssetRef::<Mesh>::Asset(asset).uuid(), asset);
    assert_eq!(AssetRef::<Mesh>::Made(made).uuid(), made);
    assert_eq!(
        AssetRef::<Mesh>::Local(sphere.clone()).uuid(),
        LocalSource::<Mesh>::uuid(&sphere)
    );
}

/// Only a value made in code is made: an asset and a local source are not.
#[test]
fn only_a_made_reference_is_made() {
    let cube = ProceduralMesh::new(ProceduralMeshKind::Cube, [1.0, 0.0, 0.0, 0.0]);

    assert!(AssetRef::<Mesh>::Made(AssetUUID::new()).is_made());
    assert!(!AssetRef::<Mesh>::Asset(AssetUUID::new()).is_made());
    assert!(!AssetRef::<Mesh>::Local(cube).is_made());
}
