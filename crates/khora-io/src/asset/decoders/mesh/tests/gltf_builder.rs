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

//! Builds small `.gltf` documents in memory: JSON with one embedded
//! `data:application/octet-stream;base64,` buffer.

use base64::Engine;
use serde_json::{json, Map, Value};

const FLOAT: u32 = 5126;
const UNSIGNED_BYTE: u32 = 5121;
const UNSIGNED_SHORT: u32 = 5123;
const UNSIGNED_INT: u32 = 5125;

/// glTF primitive modes.
pub(super) const MODE_POINTS: u32 = 0;
pub(super) const MODE_LINES: u32 = 1;
pub(super) const MODE_TRIANGLE_STRIP: u32 = 5;
pub(super) const MODE_TRIANGLE_FAN: u32 = 6;

#[derive(Default)]
pub(super) struct GltfBuilder {
    bin: Vec<u8>,
    buffer_views: Vec<Value>,
    accessors: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    scenes: Vec<Value>,
    default_scene: Option<usize>,
}

impl GltfBuilder {
    pub(super) fn new() -> Self {
        Self::default()
    }

    fn view(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": bytes.len(),
        }));
        self.buffer_views.len() - 1
    }

    fn accessor(
        &mut self,
        bytes: &[u8],
        component_type: u32,
        count: usize,
        ty: &str,
        normalized: bool,
        bounds: Option<(Vec<f32>, Vec<f32>)>,
    ) -> usize {
        let view = self.view(bytes);
        let mut accessor = json!({
            "bufferView": view,
            "componentType": component_type,
            "count": count,
            "type": ty,
        });
        if normalized {
            accessor["normalized"] = json!(true);
        }
        if let Some((min, max)) = bounds {
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn floats<const N: usize>(values: &[[f32; N]]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|v| v.iter().flat_map(|f| f.to_le_bytes()))
            .collect()
    }

    /// A `POSITION` accessor (FLOAT VEC3, with the min/max glTF requires).
    pub(super) fn positions(&mut self, values: &[[f32; 3]]) -> usize {
        let mut min = vec![f32::MAX; 3];
        let mut max = vec![f32::MIN; 3];
        for v in values {
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
                max[k] = max[k].max(v[k]);
            }
        }
        self.accessor(
            &Self::floats(values),
            FLOAT,
            values.len(),
            "VEC3",
            false,
            Some((min, max)),
        )
    }

    /// A FLOAT VEC2 accessor.
    pub(super) fn vec2(&mut self, values: &[[f32; 2]]) -> usize {
        self.accessor(
            &Self::floats(values),
            FLOAT,
            values.len(),
            "VEC2",
            false,
            None,
        )
    }

    /// A FLOAT VEC3 accessor.
    pub(super) fn vec3(&mut self, values: &[[f32; 3]]) -> usize {
        self.accessor(
            &Self::floats(values),
            FLOAT,
            values.len(),
            "VEC3",
            false,
            None,
        )
    }

    /// A FLOAT VEC4 accessor.
    pub(super) fn vec4(&mut self, values: &[[f32; 4]]) -> usize {
        self.accessor(
            &Self::floats(values),
            FLOAT,
            values.len(),
            "VEC4",
            false,
            None,
        )
    }

    /// A normalised UNSIGNED_BYTE VEC4 accessor.
    pub(super) fn unorm8x4(&mut self, values: &[[u8; 4]]) -> usize {
        let bytes: Vec<u8> = values.iter().flatten().copied().collect();
        self.accessor(&bytes, UNSIGNED_BYTE, values.len(), "VEC4", true, None)
    }

    /// A normalised UNSIGNED_SHORT VEC4 accessor.
    pub(super) fn unorm16x4(&mut self, values: &[[u16; 4]]) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|v| v.iter().flat_map(|c| c.to_le_bytes()))
            .collect();
        self.accessor(&bytes, UNSIGNED_SHORT, values.len(), "VEC4", true, None)
    }

    /// An UNSIGNED_INT SCALAR index accessor.
    pub(super) fn indices(&mut self, values: &[u32]) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|i| i.to_le_bytes()).collect();
        self.accessor(&bytes, UNSIGNED_INT, values.len(), "SCALAR", false, None)
    }

    /// Adds a mesh made of `primitives` (see [`primitive`]).
    pub(super) fn mesh(&mut self, primitives: Vec<Value>) -> usize {
        self.meshes.push(json!({ "primitives": primitives }));
        self.meshes.len() - 1
    }

    /// Adds a node; `node` is its glTF JSON (`mesh`, `children`,
    /// `translation`, `rotation`, `scale`, `matrix`).
    pub(super) fn node(&mut self, node: Value) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Adds a scene listing `roots`.
    pub(super) fn scene(&mut self, roots: &[usize]) -> usize {
        self.scenes.push(json!({ "nodes": roots }));
        self.scenes.len() - 1
    }

    /// Flags `scene` as the document's default scene.
    pub(super) fn default_scene(&mut self, scene: usize) {
        self.default_scene = Some(scene);
    }

    /// The `.gltf` JSON bytes.
    pub(super) fn build(self) -> Vec<u8> {
        let mut root = Map::new();
        root.insert("asset".into(), json!({ "version": "2.0" }));
        let uri = format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&self.bin)
        );
        root.insert(
            "buffers".into(),
            json!([{ "byteLength": self.bin.len(), "uri": uri }]),
        );
        root.insert("bufferViews".into(), Value::Array(self.buffer_views));
        root.insert("accessors".into(), Value::Array(self.accessors));
        if !self.meshes.is_empty() {
            root.insert("meshes".into(), Value::Array(self.meshes));
        }
        if !self.nodes.is_empty() {
            root.insert("nodes".into(), Value::Array(self.nodes));
        }
        if !self.scenes.is_empty() {
            root.insert("scenes".into(), Value::Array(self.scenes));
        }
        if let Some(scene) = self.default_scene {
            root.insert("scene".into(), json!(scene));
        }
        serde_json::to_vec(&Value::Object(root)).expect("glTF JSON serialises")
    }
}

/// A triangle-list primitive: `attributes` are `(semantic, accessor)` pairs.
pub(super) fn primitive(attributes: &[(&str, usize)], indices: Option<usize>) -> Value {
    primitive_with_mode(attributes, indices, 4)
}

/// A primitive with an explicit glTF `mode`.
pub(super) fn primitive_with_mode(
    attributes: &[(&str, usize)],
    indices: Option<usize>,
    mode: u32,
) -> Value {
    let attributes: Map<String, Value> = attributes
        .iter()
        .map(|(semantic, accessor)| ((*semantic).to_string(), json!(accessor)))
        .collect();
    let mut primitive = json!({ "attributes": attributes, "mode": mode });
    if let Some(indices) = indices {
        primitive["indices"] = json!(indices);
    }
    primitive
}
