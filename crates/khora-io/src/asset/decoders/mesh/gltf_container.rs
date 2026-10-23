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

//! A glTF file's sub-assets: its scenes, primitives, materials and textures.
//!
//! The file's own uuid still decodes as one merged [`Mesh`] (see
//! [`GltfDecoder`](super::GltfDecoder)); each piece of it is also an asset of
//! its own, named by a label inside the file:
//!
//! | label | asset |
//! |---|---|
//! | `scene` | the default scene (else the first; a file without scenes shows each mesh once) |
//! | `scene{i}` | scene `i` |
//! | `mesh{m}/prim{p}` | primitive `p` of mesh `m`, in the mesh's own space |
//! | `material{i}` | material `i`, as a [`StandardMaterial`] |
//! | `material_default` | the glTF default material, for primitives without one |
//! | `texture{i}` | the image of texture `i` |
//!
//! Labels follow the file's order, like the glTF indices they come from.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail};
use khora_core::asset::{AlphaMode, AssetUUID, Material, StandardMaterial};
use khora_core::math::{LinearRgba, Mat4};

use super::gltf::{merge, parse_gltf, read_primitive};
use super::gltf_resources::{image_bytes, load_buffers};
use super::gltf_scene::scene_record;
use super::{FileSystemResolver, GltfResourceResolver, MeshDispatcher};
use crate::asset::{
    AssetContainer, AssetDecoder, PrefabAsset, SubAssetEntry, SubAssetListerRegistration,
    TextureDecoder,
};

/// The glTF extensions whose data the sub-assets carry. Any other extension a
/// file uses is reported once when the file is opened.
const MAPPED_EXTENSIONS: &[&str] = &["KHR_materials_emissive_strength", "KHR_materials_specular"];

/// Lists the sub-assets of a glTF file from its JSON alone: no buffer or image
/// is read. Bytes that are not glTF (an OBJ) have none.
pub fn list_gltf_sub_assets(bytes: &[u8]) -> anyhow::Result<Vec<SubAssetEntry>> {
    if !MeshDispatcher::is_gltf(bytes) {
        return Ok(Vec::new());
    }
    let gltf =
        parse_gltf(bytes).map_err(|e| anyhow!("parsing the glTF to list its sub-assets: {e}"))?;
    let document = &gltf.document;

    let entry = |label: String, type_name: &str| SubAssetEntry {
        label,
        type_name: type_name.to_owned(),
    };
    let mut entries = vec![entry("scene".to_owned(), "prefab")];
    entries.extend((0..document.scenes().len()).map(|i| entry(format!("scene{i}"), "prefab")));
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            entries.push(entry(
                format!("mesh{}/prim{}", mesh.index(), primitive.index()),
                "mesh",
            ));
        }
    }
    entries
        .extend((0..document.materials().len()).map(|i| entry(format!("material{i}"), "material")));
    if uses_default_material(document) {
        entries.push(entry("material_default".to_owned(), "material"));
    }
    entries.extend((0..document.textures().len()).map(|i| entry(format!("texture{i}"), "texture")));
    Ok(entries)
}

inventory::submit! {
    SubAssetListerRegistration {
        type_name: "mesh",
        list: list_gltf_sub_assets,
    }
}

/// Opens glTF files and extracts their sub-assets.
pub struct GltfContainer {
    resolver: Arc<dyn GltfResourceResolver>,
}

impl GltfContainer {
    /// A container reading external files under `assets_root`.
    pub fn new(assets_root: impl Into<PathBuf>) -> Self {
        Self {
            resolver: Arc::new(FileSystemResolver::new(assets_root.into())),
        }
    }
}

/// A glTF file, parsed and validated once, with every buffer it points at.
struct OpenedGltf {
    document: gltf::Document,
    buffers: Vec<Vec<u8>>,
    /// The file's folder, relative to the assets root: external images are
    /// read from there.
    base_dir: PathBuf,
    /// The file's name without its extension: the name of a scene that has
    /// none.
    file_stem: String,
}

impl AssetContainer for GltfContainer {
    fn open(&self, bytes: &[u8], source_path: &Path) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        if !MeshDispatcher::is_gltf(bytes) {
            bail!(
                "'{}' is not a glTF file, so it has no sub-assets",
                source_path.display()
            );
        }
        let gltf = parse_gltf(bytes)
            .map_err(|e| anyhow!("parsing glTF '{}': {e}", source_path.display()))?;
        let base_dir = source_path.parent().unwrap_or(Path::new("")).to_path_buf();
        let buffers = load_buffers(&gltf, &*self.resolver, &base_dir).map_err(|e| {
            anyhow!(
                "loading the buffers of glTF '{}': {e}",
                source_path.display()
            )
        })?;
        warn_unmapped_features(&gltf.document, source_path);
        let file_stem = source_path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Arc::new(OpenedGltf {
            document: gltf.document,
            buffers,
            base_dir,
            file_stem,
        }))
    }

    fn extract(
        &self,
        opened: &(dyn Any + Send + Sync),
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> anyhow::Result<Box<dyn Any + Send>> {
        let opened = opened.downcast_ref::<OpenedGltf>().ok_or_else(|| {
            anyhow!("glTF sub-asset '{label}': the opened container is not a glTF")
        })?;
        let piece = Label::parse(label)
            .ok_or_else(|| anyhow!("'{label}' is not a glTF sub-asset label"))?;
        if piece.type_name() != type_name {
            bail!(
                "glTF sub-asset '{label}' is a '{}', not a '{type_name}'",
                piece.type_name()
            );
        }
        let document = &opened.document;
        match piece {
            Label::Primitive { mesh, primitive } => {
                let primitive = document
                    .meshes()
                    .nth(mesh)
                    .and_then(|mesh| mesh.primitives().nth(primitive))
                    .ok_or_else(|| anyhow!("the glTF has no primitive '{label}'"))?;
                let read = read_primitive(&primitive, &Mat4::IDENTITY, &opened.buffers)
                    .map_err(|e| anyhow!("reading primitive '{label}': {e}"))?;
                let mesh =
                    merge(vec![read]).map_err(|e| anyhow!("building mesh '{label}': {e}"))?;
                Ok(Box::new(mesh))
            }
            Label::Material(Some(index)) => {
                let material = document
                    .materials()
                    .nth(index)
                    .ok_or_else(|| anyhow!("the glTF has no material '{label}'"))?;
                let material: Box<dyn Material> = Box::new(standard_material(&material, container));
                Ok(Box::new(material))
            }
            Label::Material(None) => {
                if !uses_default_material(document) {
                    bail!("the glTF has no '{label}': every primitive names a material");
                }
                let material: Box<dyn Material> = Box::new(default_material());
                Ok(Box::new(material))
            }
            Label::Texture(index) => {
                let texture = document
                    .textures()
                    .nth(index)
                    .ok_or_else(|| anyhow!("the glTF has no texture '{label}'"))?;
                let bytes = image_bytes(
                    document,
                    &texture.source(),
                    &opened.buffers,
                    &*self.resolver,
                    &opened.base_dir,
                )
                .map_err(|e| anyhow!("reading the image of '{label}': {e}"))?;
                let texture = TextureDecoder
                    .load(&bytes)
                    .map_err(|e| anyhow!("decoding the image of '{label}': {e}"))?;
                Ok(Box::new(texture))
            }
            Label::Scene(scene) => {
                let record = scene_record(document, scene, container, &opened.file_stem)?;
                Ok(Box::new(PrefabAsset(record)))
            }
        }
    }
}

/// One sub-asset label, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Label {
    /// `scene` (`None`, the default scene) or `scene{i}`.
    Scene(Option<usize>),
    /// `mesh{m}/prim{p}`.
    Primitive { mesh: usize, primitive: usize },
    /// `material{i}`, or `material_default` (`None`).
    Material(Option<usize>),
    /// `texture{i}`.
    Texture(usize),
}

impl Label {
    fn parse(label: &str) -> Option<Self> {
        match label {
            "scene" => return Some(Self::Scene(None)),
            "material_default" => return Some(Self::Material(None)),
            _ => {}
        }
        if let Some(rest) = label.strip_prefix("mesh") {
            let (mesh, primitive) = rest.split_once("/prim")?;
            return Some(Self::Primitive {
                mesh: index(mesh)?,
                primitive: index(primitive)?,
            });
        }
        if let Some(rest) = label.strip_prefix("scene") {
            return index(rest).map(|i| Self::Scene(Some(i)));
        }
        if let Some(rest) = label.strip_prefix("material") {
            return index(rest).map(|i| Self::Material(Some(i)));
        }
        label
            .strip_prefix("texture")
            .and_then(index)
            .map(Self::Texture)
    }

    /// The index type of the sub-asset this label names.
    fn type_name(self) -> &'static str {
        match self {
            Self::Scene(_) => "prefab",
            Self::Primitive { .. } => "mesh",
            Self::Material(_) => "material",
            Self::Texture(_) => "texture",
        }
    }
}

/// A decimal index written without sign or padding, as the lister writes it.
fn index(digits: &str) -> Option<usize> {
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    canonical.then(|| digits.parse().ok()).flatten()
}

/// Whether some primitive names no material, and so uses the glTF default.
fn uses_default_material(document: &gltf::Document) -> bool {
    document
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .any(|primitive| primitive.material().index().is_none())
}

/// A glTF material as a [`StandardMaterial`]: its texture slots name the
/// file's `texture{i}` sub-assets.
fn standard_material(material: &gltf::Material<'_>, container: AssetUUID) -> StandardMaterial {
    let texture = |index: usize| Some(AssetUUID::sub_asset(container, &format!("texture{index}")));
    let pbr = material.pbr_metallic_roughness();
    let [r, g, b, a] = pbr.base_color_factor();
    let [er, eg, eb] = material.emissive_factor();
    let strength = material.emissive_strength().unwrap_or(1.0);
    let alpha_cutoff = material.alpha_cutoff().unwrap_or(0.5);
    let alpha_mode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => AlphaMode::Mask(alpha_cutoff),
        gltf::material::AlphaMode::Blend => AlphaMode::Blend,
    };
    let specular = material.specular();
    let [sr, sg, sb] = specular
        .as_ref()
        .map_or([1.0; 3], |specular| specular.specular_color_factor());
    StandardMaterial {
        base_color: LinearRgba::new(r, g, b, a),
        base_color_texture: pbr
            .base_color_texture()
            .and_then(|info| texture(info.texture().index())),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        metallic_roughness_texture: pbr
            .metallic_roughness_texture()
            .and_then(|info| texture(info.texture().index())),
        normal_map: material
            .normal_texture()
            .and_then(|normal| texture(normal.texture().index())),
        occlusion_map: material
            .occlusion_texture()
            .and_then(|occlusion| texture(occlusion.texture().index())),
        emissive: LinearRgba::new(er * strength, eg * strength, eb * strength, 1.0),
        emissive_texture: material
            .emissive_texture()
            .and_then(|info| texture(info.texture().index())),
        alpha_mode,
        alpha_cutoff,
        double_sided: material.double_sided(),
        specular: specular
            .as_ref()
            .map_or(1.0, |specular| specular.specular_factor()),
        specular_color: LinearRgba::new(sr, sg, sb, 1.0),
        specular_texture: specular
            .as_ref()
            .and_then(|specular| specular.specular_texture())
            .and_then(|info| texture(info.texture().index())),
        specular_color_texture: specular
            .as_ref()
            .and_then(|specular| specular.specular_color_texture())
            .and_then(|info| texture(info.texture().index())),
    }
}

/// The material glTF gives a primitive that names none: every property at its
/// default — white, fully metallic and rough, opaque, single-sided.
fn default_material() -> StandardMaterial {
    StandardMaterial {
        base_color: LinearRgba::new(1.0, 1.0, 1.0, 1.0),
        base_color_texture: None,
        metallic: 1.0,
        roughness: 1.0,
        metallic_roughness_texture: None,
        normal_map: None,
        occlusion_map: None,
        emissive: LinearRgba::new(0.0, 0.0, 0.0, 1.0),
        emissive_texture: None,
        alpha_mode: AlphaMode::Opaque,
        alpha_cutoff: 0.5,
        double_sided: false,
        specular: 1.0,
        specular_color: LinearRgba::WHITE,
        specular_texture: None,
        specular_color_texture: None,
    }
}

/// Reports, once per file, the glTF features its sub-assets do not carry:
/// they still decode, without that feature.
fn warn_unmapped_features(document: &gltf::Document, source_path: &Path) {
    let file = source_path.display();
    let unmapped: Vec<&str> = document
        .extensions_used()
        .filter(|name| !MAPPED_EXTENSIONS.contains(name))
        .collect();
    if !unmapped.is_empty() {
        log::warn!(
            "glTF '{file}': extensions not imported: {}",
            unmapped.join(", ")
        );
    }

    let mut second_uv_set = false;
    let mut scaled_normal = false;
    let mut weakened_occlusion = false;
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        let tex_coords = [
            pbr.base_color_texture().map(|info| info.tex_coord()),
            pbr.metallic_roughness_texture()
                .map(|info| info.tex_coord()),
            material.normal_texture().map(|normal| normal.tex_coord()),
            material
                .occlusion_texture()
                .map(|occlusion| occlusion.tex_coord()),
            material.emissive_texture().map(|info| info.tex_coord()),
        ];
        let specular_tex_coords = material.specular().map(|specular| {
            [
                specular.specular_texture().map(|info| info.tex_coord()),
                specular
                    .specular_color_texture()
                    .map(|info| info.tex_coord()),
            ]
        });
        second_uv_set |= tex_coords
            .into_iter()
            .chain(specular_tex_coords.into_iter().flatten())
            .flatten()
            .any(|set| set != 0);
        scaled_normal |= material
            .normal_texture()
            .is_some_and(|normal| normal.scale() != 1.0);
        weakened_occlusion |= material
            .occlusion_texture()
            .is_some_and(|occlusion| occlusion.strength() != 1.0);
    }
    if second_uv_set {
        log::warn!("glTF '{file}': textures on a UV set other than 0 are sampled with UV set 0");
    }
    if scaled_normal {
        log::warn!("glTF '{file}': normal texture scale is not imported (read as 1)");
    }
    if weakened_occlusion {
        log::warn!("glTF '{file}': occlusion texture strength is not imported (read as 1)");
    }
    let weighted = document.nodes().any(|node| node.weights().is_some())
        || document.meshes().any(|mesh| mesh.weights().is_some());
    if document.cameras().len() > 0 || document.skins().len() > 0 || weighted {
        log::warn!(
            "glTF '{file}': cameras, skins and morph target weights are not imported; their \
             nodes import as plain entities"
        );
    }
    if document.samplers().len() > 0 {
        log::warn!(
            "glTF '{file}': texture samplers are not imported; every texture repeats with \
             linear filtering"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_parse_back_to_what_the_lister_writes() {
        assert_eq!(Label::parse("scene"), Some(Label::Scene(None)));
        assert_eq!(Label::parse("scene2"), Some(Label::Scene(Some(2))));
        assert_eq!(
            Label::parse("mesh3/prim0"),
            Some(Label::Primitive {
                mesh: 3,
                primitive: 0
            })
        );
        assert_eq!(Label::parse("material10"), Some(Label::Material(Some(10))));
        assert_eq!(
            Label::parse("material_default"),
            Some(Label::Material(None))
        );
        assert_eq!(Label::parse("texture0"), Some(Label::Texture(0)));
    }

    #[test]
    fn labels_the_lister_never_writes_do_not_parse() {
        for label in [
            "",
            "bogus",
            "mesh",
            "mesh1",
            "mesh1/prim",
            "mesh+1/prim0",
            "mesh01/prim0",
            "texture-1",
            "texture 1",
            "scene_",
            "material",
        ] {
            assert_eq!(Label::parse(label), None, "{label:?}");
        }
    }
}
