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

//! Builds small glTF documents in memory: `.gltf` JSON with its buffer
//! embedded as a `data:application/octet-stream;base64,` URI or referenced as
//! an external file, or a binary `.glb`. Materials, textures, samplers and
//! images (in a buffer view, a `data:` URI or an external file) are written
//! as given.

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
    materials: Vec<Value>,
    textures: Vec<Value>,
    images: Vec<Value>,
    samplers: Vec<Value>,
    extensions_used: Vec<String>,
    /// Top-level properties written as given (`cameras`, `skins`,
    /// `extensions`, …).
    top_level: Map<String, Value>,
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

    /// Adds a mesh named `name` made of `primitives`.
    pub(super) fn mesh_named(&mut self, name: &str, primitives: Vec<Value>) -> usize {
        self.meshes
            .push(json!({ "name": name, "primitives": primitives }));
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

    /// Adds a scene named `name` listing `roots`.
    pub(super) fn scene_named(&mut self, name: &str, roots: &[usize]) -> usize {
        self.scenes.push(json!({ "name": name, "nodes": roots }));
        self.scenes.len() - 1
    }

    /// Writes the top-level property `key` of the document as `value`.
    pub(super) fn top_level(&mut self, key: &str, value: Value) {
        self.top_level.insert(key.to_string(), value);
    }

    /// Flags `scene` as the document's default scene.
    pub(super) fn default_scene(&mut self, scene: usize) {
        self.default_scene = Some(scene);
    }

    /// Adds a material; `material` is its glTF JSON
    /// (`pbrMetallicRoughness`, `normalTexture`, `alphaMode`, …).
    pub(super) fn material(&mut self, material: Value) -> usize {
        self.materials.push(material);
        self.materials.len() - 1
    }

    /// Adds an image stored in a buffer view of the file's buffer.
    pub(super) fn image_in_buffer(&mut self, bytes: &[u8], mime_type: &str) -> usize {
        let view = self.view(bytes);
        self.images
            .push(json!({ "bufferView": view, "mimeType": mime_type }));
        self.images.len() - 1
    }

    /// Adds a PNG image embedded as a `data:image/png;base64,` URI.
    pub(super) fn image_data_uri(&mut self, png: &[u8]) -> usize {
        let uri = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        self.image_uri(&uri)
    }

    /// Adds an image read from `uri` (an external file, written as given).
    pub(super) fn image_uri(&mut self, uri: &str) -> usize {
        self.images.push(json!({ "uri": uri }));
        self.images.len() - 1
    }

    /// Adds a sampler; `sampler` is its glTF JSON.
    pub(super) fn sampler(&mut self, sampler: Value) -> usize {
        self.samplers.push(sampler);
        self.samplers.len() - 1
    }

    /// Adds a texture showing `image`, sampled by `sampler` if any.
    pub(super) fn texture(&mut self, image: usize, sampler: Option<usize>) -> usize {
        let mut texture = json!({ "source": image });
        if let Some(sampler) = sampler {
            texture["sampler"] = json!(sampler);
        }
        self.textures.push(texture);
        self.textures.len() - 1
    }

    /// Lists `name` in the document's `extensionsUsed`.
    pub(super) fn extension_used(&mut self, name: &str) {
        if !self.extensions_used.iter().any(|used| used == name) {
            self.extensions_used.push(name.to_string());
        }
    }

    /// The `.gltf` JSON bytes, the buffer embedded as a `data:` URI.
    pub(super) fn build(self) -> Vec<u8> {
        let uri = format!(
            "data:application/octet-stream;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&self.bin)
        );
        let buffer = json!({ "byteLength": self.bin.len(), "uri": uri });
        let (root, _) = self.document(buffer);
        serde_json::to_vec(&root).expect("glTF JSON serialises")
    }

    /// The `.gltf` JSON bytes referencing its buffer as the external file
    /// `uri` (written as given), and that buffer's bytes.
    pub(super) fn build_with_buffer_uri(self, uri: &str) -> (Vec<u8>, Vec<u8>) {
        let buffer = json!({ "byteLength": self.bin.len(), "uri": uri });
        let (root, bin) = self.document(buffer);
        (
            serde_json::to_vec(&root).expect("glTF JSON serialises"),
            bin,
        )
    }

    /// The binary `.glb` bytes: a JSON chunk, then the buffer as the BIN chunk.
    pub(super) fn build_glb(self) -> Vec<u8> {
        const GLB_VERSION: u32 = 2;
        const CHUNK_JSON: u32 = 0x4E4F_534A;
        const CHUNK_BIN: u32 = 0x004E_4942;

        let buffer = json!({ "byteLength": self.bin.len() });
        let (root, mut bin) = self.document(buffer);
        let mut json = serde_json::to_vec(&root).expect("glTF JSON serialises");
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();

        let mut glb = Vec::with_capacity(total);
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&GLB_VERSION.to_le_bytes());
        glb.extend_from_slice(&(total as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_JSON.to_le_bytes());
        glb.extend_from_slice(&json);
        glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        glb.extend_from_slice(&CHUNK_BIN.to_le_bytes());
        glb.extend_from_slice(&bin);
        glb
    }

    /// The document's JSON root with `buffer` as its only buffer, and the
    /// buffer's bytes.
    fn document(self, buffer: Value) -> (Value, Vec<u8>) {
        let mut root = Map::new();
        root.insert("asset".into(), json!({ "version": "2.0" }));
        if !self.extensions_used.is_empty() {
            root.insert("extensionsUsed".into(), json!(self.extensions_used));
        }
        root.insert("buffers".into(), json!([buffer]));
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
        if !self.materials.is_empty() {
            root.insert("materials".into(), Value::Array(self.materials));
        }
        if !self.textures.is_empty() {
            root.insert("textures".into(), Value::Array(self.textures));
        }
        if !self.images.is_empty() {
            root.insert("images".into(), Value::Array(self.images));
        }
        if !self.samplers.is_empty() {
            root.insert("samplers".into(), Value::Array(self.samplers));
        }
        root.extend(self.top_level);
        (Value::Object(root), self.bin)
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

/// `primitive` drawn with the material `material`.
pub(super) fn with_material(mut primitive: Value, material: usize) -> Value {
    primitive["material"] = json!(material);
    primitive
}

/// A `width` × `height` PNG filled with `rgba`.
pub(super) fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("a PNG encodes");
    bytes
}
