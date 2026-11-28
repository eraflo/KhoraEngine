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

//! The asset index builder: a project's `assets/` scanned into a sorted [`AssetMetadata`] list.
//!
//! See the book: reference/formats.md, "Determinism".

use crate::asset::container::{SubAssetEntry, SubAssetListerRegistration};
use crate::asset::dependencies::{extract_dependencies, type_has_dependency_extractor};
use crate::asset::extensions::kind_of_path;
use crate::asset::id_registry::AssetIdRegistry;
use crate::trust::FileGuard;
use anyhow::{anyhow, Context, Result};
use khora_core::asset::{asset_key, AssetKind, AssetMetadata, AssetSource, AssetUUID};
use std::{collections::HashMap, path::PathBuf};

/// The names, lower case, of the files an operating system writes into any folder.
///
/// Windows' thumbnail cache and folder settings, macOS's folder settings.
const SYSTEM_FILES: &[&str] = &["thumbs.db", "desktop.ini", ".ds_store"];

/// A recursive scanner of a project's `assets/` directory into the index's `AssetMetadata`.
///
/// Its output is sorted by forward-slash relative path, so byte-deterministic.
pub struct IndexBuilder<'a> {
    /// What every file the index looks into is read through; its root is the
    /// project's `assets/`.
    guard: FileGuard,
    registry: Option<&'a AssetIdRegistry>,
}

impl<'a> IndexBuilder<'a> {
    /// Returns a builder over the root of `guard`, a project's `assets/` directory, reading through it.
    ///
    /// The files it looks into, a material's for its dependencies and a
    /// container's for its sub-assets, are read at the guard's trust and its
    /// refusals reported to the guard's. Paths, hence UUIDs, are relative to
    /// the root, not to the project root. Without [`Self::with_registry`], each
    /// UUID is its path's `new_v5` default. See the book: concepts/trust.md,
    /// "The guard".
    pub fn new(guard: FileGuard) -> Self {
        Self {
            guard,
            registry: None,
        }
    }

    /// Resolves each asset's UUID through `registry`, frozen or `new_v5` default.
    ///
    /// A dependency named by path resolves through it too, so it names the UUID
    /// its target is indexed under. The same registry in development and in the
    /// pack builder gives the same UUIDs.
    pub fn with_registry(mut self, registry: &'a AssetIdRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Returns the metadata of every asset under the root, sorted by forward-slash path.
    ///
    /// A file of no asset kind (a README, a glTF's `.bin`) is left out: a
    /// container reads its external files itself. Empty when the root
    /// does not exist, as in a fresh project.
    ///
    /// # Errors
    ///
    /// None is returned: a file that cannot be read is listed without its
    /// dependencies, the failure logged.
    pub fn build_metadata(&self) -> Result<Vec<AssetMetadata>> {
        let (guard, assets_root) = (&self.guard, self.guard.root());
        if !assets_root.exists() {
            return Ok(Vec::new());
        }

        let mut scanned: Vec<Scanned> = Vec::new();
        for rel in crate::fs::files_under(assets_root) {
            let file_name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if should_skip_file(file_name) {
                continue;
            }
            let Some(kind) = kind_of_path(&rel) else {
                continue;
            };
            scanned.push(Scanned {
                key: asset_key(&rel),
                kind,
                rel,
            });
        }

        // Sorted before dependencies are extracted, so the visiting order never
        // affects the result.
        scanned.sort_by(|a, b| a.key.cmp(&b.key));

        let mut metadata = Vec::with_capacity(scanned.len());
        for file in scanned {
            let uuid = self.uuid_of(&file.key);
            // Only a kind with an extractor is read: a texture or a sound never
            // is, so the scan stays one stat per file.
            let dependencies = if type_has_dependency_extractor(file.kind.name()) {
                match guard.read(&file.rel) {
                    Ok(bytes) => {
                        extract_dependencies(file.kind.name(), &file.key, &bytes, &|rel| {
                            self.uuid_of(rel)
                        })
                    }
                    Err(e) => {
                        log::warn!(
                            "asset index: failed to read '{}' for dependency extraction: {e}",
                            file.key
                        );
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            let mut variants = HashMap::with_capacity(1);
            variants.insert(
                AssetMetadata::DEFAULT_VARIANT.to_owned(),
                AssetSource::Path(file.rel.clone()),
            );
            metadata.push(AssetMetadata {
                uuid,
                source_path: file.rel.clone(),
                kind: file.kind,
                dependencies,
                variants,
                tags: Vec::new(),
            });
            push_sub_assets(&mut metadata, uuid, &file, guard);
        }
        Ok(metadata)
    }

    /// Returns the metadata encoded as a loose project's `index.bin`, its external-files table empty.
    ///
    /// # Errors
    ///
    /// When the metadata cannot be encoded.
    pub fn build_index_bytes(&self) -> Result<Vec<u8>> {
        let metadata = self.build_metadata()?;
        crate::asset::AssetIndex::encode(&metadata, &[])
            .map_err(|e| anyhow!("Failed to encode asset index: {}", e))
            .context("IndexBuilder::build_index_bytes")
    }

    /// Returns the UUID of the file at the forward-slash path `rel_fwd` under the root.
    ///
    /// Through the registry when there is one, else the path's `new_v5` default.
    /// A file's own entry and every dependency naming it by path take it here.
    fn uuid_of(&self, rel_fwd: &str) -> AssetUUID {
        match self.registry {
            Some(registry) => registry.resolve(rel_fwd),
            None => AssetUUID::new_v5(rel_fwd),
        }
    }
}

/// Returns whether a file named `name` is left out of the index and of hot-reload events.
///
/// [Scratch files](crate::fs::is_scratch), hidden files (`.gitkeep`), backups
/// (`*.bak`, `~*`), and the files the operating system writes (`Thumbs.db`,
/// `desktop.ini`, `.DS_Store`, in any case), whatever kind their extension names.
pub fn should_skip_file(name: &str) -> bool {
    name.is_empty()
        || crate::fs::is_scratch(name)
        || name.starts_with('.')
        || name.starts_with('~')
        || name.ends_with(".bak")
        || SYSTEM_FILES.contains(&name.to_ascii_lowercase().as_str())
}

/// A file the scan found, before it is sorted and read.
struct Scanned {
    /// Its forward-slash path under the root: its key and sort order.
    key: String,
    /// Its path under the root, read when its kind has an extractor or a lister.
    rel: PathBuf,
    /// Its kind, by its extension.
    kind: AssetKind,
}

/// Appends one entry per sub-asset the listers of the file's kind find in it.
///
/// Only a kind with a lister is read; a lister that fails is logged, the file's
/// own entry left as it is. A sub-asset of a kind no registration declares
/// is logged and left out.
fn push_sub_assets(
    metadata: &mut Vec<AssetMetadata>,
    container: AssetUUID,
    file: &Scanned,
    guard: &FileGuard,
) {
    let (rel_fwd, type_name) = (&file.key, file.kind.name());
    let mut listers = inventory::iter::<SubAssetListerRegistration>
        .into_iter()
        .filter(|lister| lister.type_name == type_name)
        .peekable();
    if listers.peek().is_none() {
        return;
    }
    let bytes = match guard.read(&file.rel) {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("asset index: failed to read '{rel_fwd}' to list its sub-assets: {e}");
            return;
        }
    };
    for lister in listers {
        let entries = match (lister.list)(&bytes) {
            Ok(entries) => entries,
            Err(e) => {
                log::warn!("asset index: listing the sub-assets of '{rel_fwd}' failed: {e:#}");
                continue;
            }
        };
        for SubAssetEntry { label, type_name } in entries {
            let Some(kind) = AssetKind::named(&type_name) else {
                log::warn!(
                    "asset index: '{rel_fwd}' holds '{label}' of the unknown kind '{type_name}'"
                );
                continue;
            };
            let mut variants = HashMap::with_capacity(1);
            variants.insert(
                AssetMetadata::DEFAULT_VARIANT.to_owned(),
                AssetSource::Sub {
                    container,
                    label: label.clone(),
                },
            );
            metadata.push(AssetMetadata {
                uuid: AssetUUID::sub_asset(container, &label),
                source_path: file.rel.clone(),
                kind,
                dependencies: Vec::new(),
                variants,
                tags: Vec::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::tests::kinds::{FAILING_CONTAINER, TEST_CONTAINER};
    use crate::asset::AssetIndex;
    use khora_core::asset::kinds;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn index_builder_honours_frozen_registry_uuid() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("meshes")).unwrap();
        fs::write(root.join("meshes").join("hero.gltf"), b"GLTF").unwrap();

        // Default (no registry): UUID is derived from the path.
        let default_uuid = AssetUUID::new_v5("meshes/hero.gltf");
        let md = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        assert_eq!(md[0].uuid, default_uuid);

        // Freeze a *different* identity for that path — exactly what a rename
        // does (the file kept the UUID it had under its previous name).
        let mut reg = AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ));
        let frozen = AssetUUID::new_v5("meshes/protagonist.gltf");
        reg.freeze("meshes/hero.gltf", frozen);

        let md2 = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .with_registry(&reg)
        .build_metadata()
        .unwrap();
        assert_eq!(
            md2[0].uuid, frozen,
            "IndexBuilder must resolve the frozen registry UUID, not new_v5(path)"
        );
        assert_ne!(md2[0].uuid, default_uuid);
    }

    /// **A script's dependency is the UUID the index gives the module it imports**,
    /// the frozen one of a renamed module.
    #[test]
    fn a_script_depends_on_the_uuid_its_renamed_module_is_indexed_under() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("scripts").join("ai")).unwrap();
        fs::write(
            root.join("scripts").join("damage.erg"),
            b"fn void Hit() { }",
        )
        .unwrap();
        fs::write(
            root.join("scripts").join("ai").join("guard.erg"),
            br#"import "damage.erg"; fn void Main() { }"#,
        )
        .unwrap();
        let mut reg = AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ));
        reg.rename("scripts/hurt.erg", "scripts/damage.erg");

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .with_registry(&reg)
        .build_metadata()
        .unwrap();

        let by_key = |key: &str| {
            metadata
                .iter()
                .find(|m| asset_key(&m.source_path) == key)
                .unwrap_or_else(|| panic!("{key} is indexed: {metadata:#?}"))
        };
        let module = by_key("scripts/damage.erg");
        assert_eq!(module.uuid, AssetUUID::new_v5("scripts/hurt.erg"));
        assert_eq!(
            by_key("scripts/ai/guard.erg").dependencies,
            vec![module.uuid]
        );
    }

    /// **The files the operating system writes are never assets**, in any case.
    ///
    /// Windows' `Thumbs.db` and `desktop.ini`, macOS's `.DS_Store`, whatever
    /// their extension names.
    #[test]
    fn the_files_the_system_writes_are_skipped() {
        for name in [
            "Thumbs.db",
            "thumbs.db",
            "THUMBS.DB",
            "desktop.ini",
            "Desktop.ini",
            ".DS_Store",
        ] {
            assert!(should_skip_file(name), "{name}");
        }
        for name in ["thumbs.png", "desktop.kscene", "my.db"] {
            assert!(!should_skip_file(name), "{name}");
        }
    }

    /// **Each asset is indexed under its kind; scratch files are left out.**
    #[test]
    fn build_metadata_indexes_each_asset_by_its_kind() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("scenes")).unwrap();
        fs::write(root.join("textures").join("foo.png"), b"PNG").unwrap();
        fs::write(root.join("scenes").join("default.kscene"), b"SCN").unwrap();
        // Editor / OS scratch — these MUST still be skipped.
        fs::write(root.join(".DS_Store"), b"junk").unwrap();
        fs::write(root.join("textures").join("foo.png.tmp"), b"junk").unwrap();
        fs::write(root.join("textures").join("foo.png~"), b"junk").unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        let by_path: std::collections::HashMap<String, AssetKind> = metadata
            .iter()
            .map(|m| (asset_key(&m.source_path), m.kind))
            .collect();

        assert_eq!(by_path.get("textures/foo.png"), Some(&kinds::TEXTURE));
        assert_eq!(by_path.get("scenes/default.kscene"), Some(&kinds::SCENE));
        assert_eq!(metadata.len(), 2, "scratch files are dropped: {by_path:?}");
    }

    #[test]
    fn build_metadata_is_deterministic() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("audio")).unwrap();
        fs::write(root.join("textures").join("a.png"), b"a").unwrap();
        fs::write(root.join("textures").join("b.png"), b"b").unwrap();
        fs::write(root.join("audio").join("c.wav"), b"c").unwrap();

        let bytes_a = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let bytes_b = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "two consecutive builds must be byte-equal"
        );
    }

    #[test]
    fn build_index_bytes_round_trip_through_vfs() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::write(root.join("textures").join("foo.png"), b"PNG").unwrap();

        let bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let vfs = AssetIndex::new(&bytes).expect("VFS must decode our index");
        assert_eq!(vfs.asset_count(), 1);
        let uuid = AssetUUID::new_v5("textures/foo.png");
        let meta = vfs.get_metadata(&uuid).expect("VFS must surface the asset");
        assert_eq!(meta.kind, kinds::TEXTURE);
    }

    /// Writes a `StandardMaterial` to `path` as `.kmat` RON — the on-disk form
    /// the index builder reads when extracting dependencies.
    fn write_kmat(path: &Path, material: &khora_core::asset::StandardMaterial) {
        let json =
            khora_data::scene::material_to_json(material).expect("material serializes to JSON");
        let ron = ron::ser::to_string(&json).expect("material JSON encodes to RON");
        fs::write(path, ron).unwrap();
    }

    #[test]
    fn material_metadata_lists_its_textures_sorted_and_deduped() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("materials")).unwrap();
        fs::write(root.join("textures").join("base.png"), b"PNG").unwrap();
        fs::write(root.join("textures").join("normal.png"), b"PNG").unwrap();

        // The UUIDs a material stores are derived from the texture's
        // forward-slash relative path — identical to the UUIDs the index
        // assigns those texture files.
        let base_uuid = AssetUUID::new_v5("textures/base.png");
        let normal_uuid = AssetUUID::new_v5("textures/normal.png");
        let material = khora_core::asset::StandardMaterial {
            base_color_texture: Some(base_uuid),
            // Reuse the base texture in a second slot to exercise dedup.
            metallic_roughness_texture: Some(base_uuid),
            normal_map: Some(normal_uuid),
            ..Default::default()
        };
        write_kmat(&root.join("materials").join("wood.kmat"), &material);

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        let mat_meta = metadata
            .iter()
            .find(|m| m.kind == kinds::MATERIAL)
            .expect("the material must be indexed");

        let mut expected = vec![base_uuid, normal_uuid];
        expected.sort();
        assert_eq!(
            mat_meta.dependencies, expected,
            "material deps must be both textures, sorted and deduped"
        );

        // The texture entries themselves must carry no dependencies — they are
        // leaf assets and are never read/parsed.
        for tex in metadata.iter().filter(|m| m.kind == kinds::TEXTURE) {
            assert!(
                tex.dependencies.is_empty(),
                "texture '{}' must have no dependencies",
                asset_key(&tex.source_path)
            );
        }
    }

    #[test]
    fn build_index_bytes_is_byte_equal_with_material_and_textures() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("materials")).unwrap();
        fs::write(root.join("textures").join("a.png"), b"A").unwrap();
        fs::write(root.join("textures").join("b.png"), b"B").unwrap();

        let material = khora_core::asset::StandardMaterial {
            base_color_texture: Some(AssetUUID::new_v5("textures/a.png")),
            normal_map: Some(AssetUUID::new_v5("textures/b.png")),
            ..Default::default()
        };
        write_kmat(&root.join("materials").join("m.kmat"), &material);

        let bytes_a = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let bytes_b = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "an index over a material + textures must be byte-deterministic"
        );
    }

    #[test]
    fn corrupt_material_still_builds_with_empty_deps() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("materials")).unwrap();
        fs::write(
            root.join("materials").join("broken.kmat"),
            b"this is not valid kmat RON",
        )
        .unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .expect("a corrupt material must not abort the index build");
        let mat_meta = metadata
            .iter()
            .find(|m| m.kind == kinds::MATERIAL)
            .expect("the corrupt material is still indexed");
        assert!(
            mat_meta.dependencies.is_empty(),
            "a corrupt material yields empty deps, not a panic"
        );
    }

    #[test]
    fn build_metadata_empty_root_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            dir.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        assert!(metadata.is_empty());
    }

    #[test]
    fn build_metadata_missing_root_returns_empty_vec() {
        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            Path::new("/__nonexistent__"),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        assert!(metadata.is_empty());
    }

    /// **A file that is no asset is not indexed**: a README, a file with no
    /// extension, and a glTF's `.bin`, which is its container's external file.
    #[test]
    fn a_file_that_is_no_asset_is_not_indexed() {
        use crate::asset::{AssetContainer, GltfContainer};

        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        let gltf = br#"{"asset":{"version":"2.0"},"buffers":[{"uri":"a.bin","byteLength":4}]}"#;
        fs::write(root.join("models").join("a.gltf"), gltf).unwrap();
        fs::write(root.join("models").join("a.bin"), [0u8; 4]).unwrap();
        fs::write(root.join("textures").join("a.png"), b"PNG").unwrap();
        fs::write(root.join("docs").join("README.md"), b"# Assets").unwrap();
        fs::write(root.join("notes"), b"raw").unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        let indexed: std::collections::BTreeSet<String> =
            metadata.iter().map(|m| asset_key(&m.source_path)).collect();
        assert_eq!(
            indexed,
            ["models/a.gltf", "textures/a.png"]
                .map(str::to_owned)
                .into(),
            "only the assets are indexed"
        );

        let container = GltfContainer::new(root);
        let opened = container
            .open(gltf, Path::new("models/a.gltf"))
            .expect("the glTF opens");
        assert_eq!(
            container.external_files(&*opened),
            ["models/a.bin"],
            "the buffer is the glTF's external file"
        );
    }

    // ── Sub-assets ──────────────────────────────────────────────────────────

    /// The kind name the test container's lister is keyed by.
    const TEST_CONTAINER_TYPE: &str = TEST_CONTAINER.name();

    /// The kind name the failing container's lister is keyed by.
    const FAILING_CONTAINER_TYPE: &str = FAILING_CONTAINER.name();

    /// Lists one sub-asset per non-empty line `"<type> <label>"`.
    fn list_test_container(bytes: &[u8]) -> Result<Vec<crate::asset::SubAssetEntry>> {
        let text = std::str::from_utf8(bytes)?;
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let (type_name, label) = line
                    .split_once(' ')
                    .ok_or_else(|| anyhow!("malformed test container line: {line:?}"))?;
                Ok(crate::asset::SubAssetEntry {
                    label: label.to_string(),
                    type_name: type_name.to_string(),
                })
            })
            .collect()
    }

    fn list_failing_container(_bytes: &[u8]) -> Result<Vec<crate::asset::SubAssetEntry>> {
        Err(anyhow!("this container cannot be listed"))
    }

    inventory::submit! {
        crate::asset::SubAssetListerRegistration {
            type_name: TEST_CONTAINER_TYPE,
            list: list_test_container,
        }
    }

    inventory::submit! {
        crate::asset::SubAssetListerRegistration {
            type_name: FAILING_CONTAINER_TYPE,
            list: list_failing_container,
        }
    }

    const HERO_CONTAINER: &[u8] = b"mesh mesh/Body\nmaterial material/Skin\n";

    fn find(metadata: &[AssetMetadata], uuid: AssetUUID) -> &AssetMetadata {
        metadata
            .iter()
            .find(|m| m.uuid == uuid)
            .unwrap_or_else(|| panic!("no entry with uuid {uuid} in the index"))
    }

    fn assert_sub_entry(
        metadata: &[AssetMetadata],
        container: AssetUUID,
        container_path: &Path,
        label: &str,
        type_name: &str,
    ) {
        let sub = find(metadata, AssetUUID::sub_asset(container, label));
        assert_eq!(sub.kind.name(), type_name, "kind of sub '{label}'");
        assert_eq!(
            sub.source_path, container_path,
            "a sub entry's source path is its container's path"
        );
        assert!(sub.dependencies.is_empty(), "sub '{label}' has no deps");
        assert!(sub.tags.is_empty(), "sub '{label}' has no tags");
        assert_eq!(sub.variants.len(), 1, "sub '{label}' has one variant");
        match sub.default_source() {
            Some(AssetSource::Sub {
                container: c,
                label: l,
            }) => {
                assert_eq!(*c, container, "sub '{label}' names its container");
                assert_eq!(l, label, "sub '{label}' carries its label");
            }
            other => panic!("sub '{label}' default variant must be Sub, got {other:?}"),
        }
    }

    #[test]
    fn index_lists_one_sub_entry_per_listed_sub_asset() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::create_dir_all(root.join("textures")).unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();
        // Same text, but a texture: no lister of its type, so no sub entries.
        fs::write(root.join("textures").join("a.png"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        assert_eq!(
            metadata.len(),
            4,
            "the container, its two subs and the texture: {metadata:#?}"
        );

        // The file's own entry is unchanged.
        let rel = Path::new("models").join("hero.ksubtest");
        let file_uuid = AssetUUID::new_v5("models/hero.ksubtest");
        let own = find(&metadata, file_uuid);
        assert_eq!(own.kind, TEST_CONTAINER);
        assert_eq!(own.source_path, rel);
        assert!(matches!(
            own.default_source(),
            Some(AssetSource::Path(p)) if *p == rel
        ));

        assert_sub_entry(&metadata, file_uuid, &rel, "mesh/Body", "mesh");
        assert_sub_entry(&metadata, file_uuid, &rel, "material/Skin", "material");

        let texture = find(&metadata, AssetUUID::new_v5("textures/a.png"));
        assert_eq!(texture.kind, kinds::TEXTURE);
    }

    /// **A sub-asset of a kind no registration declares is left out**; its
    /// container and its other parts are indexed.
    #[test]
    fn a_sub_asset_of_an_unregistered_kind_is_left_out() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(
            root.join("models").join("hero.ksubtest"),
            b"mesh mesh/Body\naudio sound/Step\nanimation anim/Walk\n",
        )
        .unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        let container = AssetUUID::new_v5("models/hero.ksubtest");
        let rel = Path::new("models").join("hero.ksubtest");
        assert_eq!(find(&metadata, container).kind, TEST_CONTAINER);
        assert_sub_entry(&metadata, container, &rel, "mesh/Body", "mesh");
        assert_eq!(
            metadata.len(),
            2,
            "the container and its mesh: {metadata:#?}"
        );
    }

    #[test]
    fn sub_entries_follow_their_file_entry() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("a.ksubtest"), b"mesh mesh/A\n").unwrap();
        fs::write(root.join("models").join("b.ksubtest"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        let a = AssetUUID::new_v5("models/a.ksubtest");
        let b = AssetUUID::new_v5("models/b.ksubtest");
        let order: Vec<AssetUUID> = metadata.iter().map(|m| m.uuid).collect();
        assert_eq!(order.len(), 5, "two files and three subs: {metadata:#?}");
        assert_eq!(order[0], a);
        assert_eq!(order[1], AssetUUID::sub_asset(a, "mesh/A"));
        assert_eq!(order[2], b);
        let mut b_subs = vec![order[3], order[4]];
        b_subs.sort();
        let mut expected = vec![
            AssetUUID::sub_asset(b, "mesh/Body"),
            AssetUUID::sub_asset(b, "material/Skin"),
        ];
        expected.sort();
        assert_eq!(b_subs, expected);
    }

    /// A rename freezes the file's old uuid in the registry; its sub uuids are
    /// derived from that frozen uuid, so they survive the rename too.
    #[test]
    fn sub_uuids_derive_from_the_registry_frozen_file_uuid() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();

        let mut reg = AssetIdRegistry::load_or_unread(&crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ));
        let frozen = AssetUUID::new_v5("models/protagonist.ksubtest");
        reg.freeze("models/hero.ksubtest", frozen);

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .with_registry(&reg)
        .build_metadata()
        .unwrap();
        assert_eq!(
            metadata.len(),
            3,
            "the file and its two subs: {metadata:#?}"
        );
        let rel = Path::new("models").join("hero.ksubtest");
        assert_eq!(find(&metadata, frozen).source_path, rel);
        assert_sub_entry(&metadata, frozen, &rel, "mesh/Body", "mesh");
        assert_sub_entry(&metadata, frozen, &rel, "material/Skin", "material");

        let path_default = AssetUUID::new_v5("models/hero.ksubtest");
        assert!(
            metadata
                .iter()
                .all(|m| m.uuid != AssetUUID::sub_asset(path_default, "mesh/Body")),
            "sub uuids must not derive from the path default once the file is frozen"
        );
    }

    #[test]
    fn a_lister_error_keeps_the_file_entry_and_spares_the_other_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("broken.ksubfail"), b"anything").unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .expect("a lister error must not abort the index build");

        let broken_rel = Path::new("models").join("broken.ksubfail");
        let broken = find(&metadata, AssetUUID::new_v5("models/broken.ksubfail"));
        assert_eq!(broken.kind, FAILING_CONTAINER);
        assert!(matches!(
            broken.default_source(),
            Some(AssetSource::Path(p)) if *p == broken_rel
        ));
        assert_eq!(
            metadata
                .iter()
                .filter(|m| m.source_path == broken_rel)
                .count(),
            1,
            "the failing file adds no sub entry"
        );

        // The next file is still listed.
        let hero_rel = Path::new("models").join("hero.ksubtest");
        let hero = AssetUUID::new_v5("models/hero.ksubtest");
        assert_sub_entry(&metadata, hero, &hero_rel, "mesh/Body", "mesh");
        assert_sub_entry(&metadata, hero, &hero_rel, "material/Skin", "material");
        assert_eq!(metadata.len(), 4);
    }

    #[test]
    fn an_index_with_sub_entries_is_deterministic() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("hero.ksubtest"), HERO_CONTAINER).unwrap();
        fs::write(root.join("models").join("villain.ksubtest"), HERO_CONTAINER).unwrap();

        let metadata = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_metadata()
        .unwrap();
        assert_eq!(metadata.len(), 6, "two files, two subs each: {metadata:#?}");

        let bytes_a = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let bytes_b = IndexBuilder::new(crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "two consecutive builds must be byte-equal"
        );

        // The `Sub` source round-trips through the index encoding.
        let vfs = AssetIndex::new(&bytes_a).expect("the index with subs decodes");
        assert_eq!(vfs.asset_count(), 6);
        let hero = AssetUUID::new_v5("models/hero.ksubtest");
        let body = vfs
            .get_metadata(&AssetUUID::sub_asset(hero, "mesh/Body"))
            .expect("the sub entry survives encoding");
        assert!(matches!(
            body.default_source(),
            Some(AssetSource::Sub { container, label }) if *container == hero && label == "mesh/Body"
        ));
    }

    // ── Reads through a guard ───────────────────────────────────────────────

    /// One mebibyte.
    const MIB: usize = 1 << 20;

    /// The strict cap of a text file — a material, or a file of a kind the
    /// trust module does not name — at `Restricted` trust.
    const STRICT_TEXT_CAP: usize = 16 * MIB;

    /// Returns `bytes` padded with `pad` to one byte over the strict text cap.
    ///
    /// It still parses as it did, but a restricted guard refuses it for its size.
    fn oversized(bytes: &[u8], pad: u8) -> Vec<u8> {
        let mut padded = bytes.to_vec();
        padded.resize(STRICT_TEXT_CAP + 1, pad);
        padded
    }

    /// A guard over `root` at `Restricted` trust, reporting to `refusals`.
    fn restricted_guard(
        root: &Path,
        refusals: &std::sync::Arc<crate::trust::Refusals>,
    ) -> crate::trust::FileGuard {
        crate::trust::FileGuard::new(
            root,
            crate::trust::Trust::Restricted,
            std::sync::Arc::clone(refusals),
        )
    }

    /// The refusals quarantined for the file named `file_name`.
    fn refused_named(
        refusals: &crate::trust::Refusals,
        file_name: &str,
    ) -> Vec<crate::trust::SecurityRefusal> {
        refusals
            .quarantined()
            .into_iter()
            .filter(|refusal| {
                refusal.path.file_name().and_then(|name| name.to_str()) == Some(file_name)
            })
            .collect()
    }

    /// **The index reads a material through its guard.**
    ///
    /// Under a restricted guard, a `.kmat` padded past the strict text cap is
    /// refused and quarantined, its entry kept without dependencies; the same
    /// material under the cap lists its texture.
    #[test]
    fn the_index_reads_a_material_through_its_guard() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("materials")).unwrap();
        let texture = AssetUUID::new_v5("textures/base.png");
        let material = khora_core::asset::StandardMaterial {
            base_color_texture: Some(texture),
            ..Default::default()
        };
        write_kmat(&root.join("materials").join("small.kmat"), &material);
        let kmat = fs::read(root.join("materials").join("small.kmat")).unwrap();
        fs::write(
            root.join("materials").join("big.kmat"),
            oversized(&kmat, b' '),
        )
        .unwrap();
        let problems = std::sync::Arc::new(khora_core::problems::Problems::default());
        let refusals = std::sync::Arc::new(crate::trust::Refusals::new(
            Some(std::sync::Arc::clone(&problems)),
            None,
        ));
        let guard = restricted_guard(root, &refusals);

        let metadata = IndexBuilder::new(guard)
            .build_metadata()
            .expect("a refused file does not abort the index");

        let big = find(&metadata, AssetUUID::new_v5("materials/big.kmat"));
        assert_eq!(big.kind, kinds::MATERIAL, "its own entry is kept");
        assert!(
            big.dependencies.is_empty(),
            "a refused material lists no dependency: {:?}",
            big.dependencies
        );
        let small = find(&metadata, AssetUUID::new_v5("materials/small.kmat"));
        assert_eq!(
            small.dependencies,
            vec![texture],
            "a material under its cap is read through the guard"
        );

        let refused = refused_named(&refusals, "big.kmat");
        assert_eq!(refused.len(), 1, "the oversized material is quarantined");
        assert_eq!(
            refused[0].reason,
            crate::trust::SecurityReason::TooLarge {
                size: (STRICT_TEXT_CAP + 1) as u64,
                cap: STRICT_TEXT_CAP as u64,
            }
        );
        assert_eq!(refused[0].trust, crate::trust::Trust::Restricted);
        assert_eq!(refused[0].layer, crate::trust::TrustLayer::File);
        assert!(
            refused_named(&refusals, "small.kmat").is_empty(),
            "the material under its cap is not refused"
        );
        assert!(
            !problems
                .of_source(&khora_core::problems::ProblemSource::Security)
                .is_empty(),
            "the refusal is reported"
        );
    }

    /// **The index reads a container through its guard.**
    ///
    /// Under a restricted guard, a container padded past the strict cap is
    /// refused and quarantined: its entry is kept and no sub-asset is listed.
    #[test]
    fn the_index_reads_a_container_through_its_guard() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(
            root.join("models").join("hero.ksubtest"),
            oversized(HERO_CONTAINER, b'\n'),
        )
        .unwrap();
        let refusals = crate::trust::Refusals::detached();
        let guard = restricted_guard(root, &refusals);

        let metadata = IndexBuilder::new(guard)
            .build_metadata()
            .expect("a refused file does not abort the index");

        let hero = AssetUUID::new_v5("models/hero.ksubtest");
        let own = find(&metadata, hero);
        assert_eq!(own.kind, TEST_CONTAINER, "its own entry is kept");
        assert_eq!(
            metadata.len(),
            1,
            "no sub-asset of a refused container is listed: {metadata:#?}"
        );
        let refused = refused_named(&refusals, "hero.ksubtest");
        assert_eq!(refused.len(), 1, "the oversized container is quarantined");
        assert!(
            matches!(
                refused[0].reason,
                crate::trust::SecurityReason::TooLarge { .. }
            ),
            "{:?}",
            refused[0].reason
        );
    }
}
