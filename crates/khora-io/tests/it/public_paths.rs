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

//! Compile-level guard over `khora_io`'s public surface, plus the paths
//! other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), and the
//! trait impls. A reorganisation that moves code between files must keep every
//! one of these paths valid, so this module stops compiling the moment one
//! disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions).
//!
//! The list was generated from `khora_io`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.
//!
//! One deliberate exception: `khora_io::serialization` re-exports eleven items
//! of `khora_data::scene` (`migrate_payload`, the four
//! `*SerializationStrategy` types, `DeserializationError`, `MigrationError`,
//! `SceneMigration`, `SceneMigrationRegistration`, `SerializationError`,
//! `SerializationStrategy`) that no crate uses; the engine-tidy plan deletes
//! them, so they are left out. `serialization::SerializationService` and the
//! rest of `serialization::service` are pinned.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_asset_decoder_box_dyn_material<
    T: khora_io::asset::AssetDecoder<Box<dyn khora_core::asset::Material>>,
>() {
}
fn is_asset_decoder_cpu_shader_source<
    T: khora_io::asset::AssetDecoder<khora_core::renderer::api::resource::CpuShaderSource>,
>() {
}
fn is_asset_decoder_cpu_texture<
    T: khora_io::asset::AssetDecoder<khora_core::renderer::api::resource::CpuTexture>,
>() {
}
fn is_asset_decoder_font<T: khora_io::asset::AssetDecoder<khora_core::asset::font::Font>>() {}
fn is_asset_decoder_mesh<
    T: khora_io::asset::AssetDecoder<khora_core::renderer::api::scene::Mesh>,
>() {
}
fn is_asset_decoder_script_module<
    T: khora_io::asset::AssetDecoder<khora_core::asset::ScriptModule>,
>() {
}
fn is_asset_decoder_sound_data<T: khora_io::asset::AssetDecoder<khora_data::assets::SoundData>>() {}
fn is_asset_io<T: khora_io::asset::AssetIo>() {}
fn is_asset_writer<T: khora_io::asset::AssetWriter>() {}
fn is_clone<T: Clone>() {}
fn is_collect<T: inventory::Collect>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_eq<T: Eq>() {}
fn is_gltf_resource_resolver<T: khora_io::asset::decoders::mesh::GltfResourceResolver>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_serialize<T: serde::Serialize>() {}
fn is_source_loader<T: khora_script::SourceLoader>() {}
fn is_supersedes<T: khora_core::event::Supersedes>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_io::asset as _;
    use khora_io::asset::audio as _;
    use khora_io::asset::decoders as _;
    use khora_io::asset::decoders::audio as _;
    use khora_io::asset::decoders::font as _;
    use khora_io::asset::decoders::material as _;
    use khora_io::asset::decoders::mesh as _;
    use khora_io::asset::decoders::script as _;
    use khora_io::asset::decoders::shader as _;
    use khora_io::asset::decoders::texture as _;
    use khora_io::asset::font as _;
    use khora_io::asset::material as _;
    use khora_io::asset::mesh as _;
    use khora_io::asset::script as _;
    use khora_io::asset::shader as _;
    use khora_io::asset::texture as _;
    use khora_io::asset_resolver as _;
    use khora_io::script_compile as _;
    use khora_io::script_hot_reload as _;
    use khora_io::script_mirror as _;
    use khora_io::serialization as _;
    use khora_io::shader_hot_reload as _;
    use khora_io::vfs as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn asset_change_event_fields(x: &khora_io::asset::AssetChangeEvent) {
    let _ = (&x.kind, &x.rel_path, &x.uuid);
}

fn asset_change_kind_variants(x: &khora_io::asset::AssetChangeKind) {
    match x {
        khora_io::asset::AssetChangeKind::Created => {}
        khora_io::asset::AssetChangeKind::Modified => {}
        khora_io::asset::AssetChangeKind::Removed => {}
    }
}

fn decoder_registration_fields(x: &khora_io::asset::DecoderRegistration) {
    let _ = (&x.type_name, &x.register);
}

fn manifest_entry_fields(x: &khora_io::asset::ManifestEntry) {
    let _ = (&x.uuid, &x.blake3, &x.size);
}

fn pack_header_fields(x: &khora_io::asset::PackHeader) {
    let _ = (&x.format_version, &x.asset_count, &x.flags);
}

fn pack_output_fields(x: &khora_io::asset::PackOutput) {
    let _ = (&x.index_bin, &x.data_pack, &x.manifest_bin, &x.asset_count);
}

fn pack_progress_variants(x: &khora_io::asset::PackProgress) {
    match x {
        khora_io::asset::PackProgress::Started { .. } => {}
        khora_io::asset::PackProgress::Finished { .. } => {}
        khora_io::asset::PackProgress::Failed { .. } => {}
    }
}

fn serialization_service_error_variants(x: &khora_io::serialization::SerializationServiceError) {
    match x {
        khora_io::serialization::SerializationServiceError::StrategyNotFound => {}
        khora_io::serialization::SerializationServiceError::InvalidHeader => {}
        khora_io::serialization::SerializationServiceError::ProcessingError(..) => {}
    }
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    fn asset_decoder_trait_items<
        T: khora_io::asset::AssetDecoder<khora_data::assets::SoundData>,
    >() {
        let _ = <T as khora_io::asset::AssetDecoder<khora_data::assets::SoundData>>::load;
    }

    fn asset_io_trait_items<T: khora_io::asset::AssetIo>() {
        let _ = <T as khora_io::asset::AssetIo>::load_bytes;
    }

    fn asset_writer_trait_items<T: khora_io::asset::AssetWriter>() {
        let _ = <T as khora_io::asset::AssetWriter>::write_bytes;
    }

    fn gltf_resource_resolver_trait_items<
        T: khora_io::asset::decoders::mesh::GltfResourceResolver,
    >() {
        let _ = <T as khora_io::asset::decoders::mesh::GltfResourceResolver>::resolve_buffer;
        let _ = <T as khora_io::asset::decoders::mesh::GltfResourceResolver>::resolve_image;
    }

    fn gltf_resource_resolver_trait_identity<T: khora_io::asset::decoders::GltfResourceResolver>() {
        gltf_resource_resolver_trait_items::<T>();
    }

    fn gltf_resource_resolver_trait_identity_rev<
        T: khora_io::asset::decoders::mesh::GltfResourceResolver,
    >() {
        gltf_resource_resolver_trait_identity::<T>();
    }

    fn gltf_resource_resolver_trait_identity_2<T: khora_io::asset::mesh::GltfResourceResolver>() {
        gltf_resource_resolver_trait_items::<T>();
    }

    fn gltf_resource_resolver_trait_identity_2_rev<
        T: khora_io::asset::decoders::mesh::GltfResourceResolver,
    >() {
        gltf_resource_resolver_trait_identity_2::<T>();
    }

    fn gltf_resource_resolver_trait_identity_3<T: khora_io::asset::GltfResourceResolver>() {
        gltf_resource_resolver_trait_items::<T>();
    }

    fn gltf_resource_resolver_trait_identity_3_rev<
        T: khora_io::asset::decoders::mesh::GltfResourceResolver,
    >() {
        gltf_resource_resolver_trait_identity_3::<T>();
    }
}

#[test]
fn module_asset_paths_still_resolve() {
    let _ = type_name::<khora_io::asset::AssetChangeEvent>();
    let _ = asset_change_event_fields as fn(&khora_io::asset::AssetChangeEvent);
    is_debug::<khora_io::asset::AssetChangeEvent>();
    is_clone::<khora_io::asset::AssetChangeEvent>();
    is_supersedes::<khora_io::asset::AssetChangeEvent>();
    let _ = type_name::<khora_io::asset::AssetChangeKind>();
    let _ = asset_change_kind_variants as fn(&khora_io::asset::AssetChangeKind);
    is_debug::<khora_io::asset::AssetChangeKind>();
    is_clone::<khora_io::asset::AssetChangeKind>();
    is_partial_eq::<khora_io::asset::AssetChangeKind>();
    is_eq::<khora_io::asset::AssetChangeKind>();
    // trait `khora_io::asset::AssetDecoder`: see `asset_decoder_trait_items`
    let _ = type_name::<khora_io::asset::AssetIdRegistry>();
    let _: fn(std::path::PathBuf) -> khora_io::asset::AssetIdRegistry =
        khora_io::asset::AssetIdRegistry::load;
    let _ = khora_io::asset::AssetIdRegistry::resolve;
    let _ = khora_io::asset::AssetIdRegistry::len;
    let _ = khora_io::asset::AssetIdRegistry::is_empty;
    let _ = khora_io::asset::AssetIdRegistry::freeze;
    let _ = khora_io::asset::AssetIdRegistry::rename;
    let _ = khora_io::asset::AssetIdRegistry::remove;
    let _ = khora_io::asset::AssetIdRegistry::file_path;
    let _ = khora_io::asset::AssetIdRegistry::ensure_file;
    let _ = khora_io::asset::AssetIdRegistry::save;
    is_debug::<khora_io::asset::AssetIdRegistry>();
    is_clone::<khora_io::asset::AssetIdRegistry>();
    // trait `khora_io::asset::AssetIo`: see `asset_io_trait_items`
    let _ = type_name::<khora_io::asset::AssetService>();
    let _ = khora_io::asset::AssetService::new;
    let _ = khora_io::asset::AssetService::vfs;
    let _: fn(
        &mut khora_io::asset::AssetService,
        &str,
        khora_io::asset::decoders::audio::WavDecoder,
    ) = khora_io::asset::AssetService::register_decoder::<khora_data::assets::SoundData>;
    let _ = khora_io::asset::AssetService::register_inventory_decoders;
    let _: fn(
        &std::path::Path,
        &khora_io::asset::AssetIdRegistry,
        std::sync::Arc<khora_telemetry::MetricsRegistry>,
    ) -> anyhow::Result<khora_io::asset::AssetService> =
        khora_io::asset::AssetService::open_loose_files;
    let _: fn(&mut khora_io::asset::AssetService, &std::path::Path) =
        khora_io::asset::AssetService::register_default_decoders;
    let _ = khora_io::asset::AssetService::load::<khora_data::assets::SoundData>;
    let _ = khora_io::asset::AssetService::load_raw;
    let _ = khora_io::asset::AssetService::invalidate;
    let _ = khora_io::asset::AssetService::reindex;
    let _ = khora_io::asset::AssetService::load_count;
    let _ = khora_io::asset::AssetService::cached_type_count;
    let _ = type_name::<khora_io::asset::AssetWatcher>();
    let _: fn(std::path::PathBuf) -> anyhow::Result<khora_io::asset::AssetWatcher> =
        khora_io::asset::AssetWatcher::new;
    let _ = khora_io::asset::AssetWatcher::assets_root;
    let _ = khora_io::asset::AssetWatcher::poll_for;
    let _ = khora_io::asset::AssetWatcher::dropped;
    // trait `khora_io::asset::AssetWriter`: see `asset_writer_trait_items`
    let _ = type_name::<khora_io::asset::DecoderRegistration>();
    let _ = decoder_registration_fields as fn(&khora_io::asset::DecoderRegistration);
    is_collect::<khora_io::asset::DecoderRegistration>();
    let _ = type_name::<khora_io::asset::DecoderRegistry>();
    let _ = khora_io::asset::DecoderRegistry::new;
    let _: fn(
        &mut khora_io::asset::DecoderRegistry,
        &str,
        khora_io::asset::decoders::audio::WavDecoder,
    ) = khora_io::asset::DecoderRegistry::register::<khora_data::assets::SoundData>;
    let _ = khora_io::asset::DecoderRegistry::decode::<khora_data::assets::SoundData>;
    let _ = khora_io::asset::EXTENSIONLESS_ASSET_TYPE;
    let _ = type_name::<khora_io::asset::FileLoader>();
    let _: fn(std::path::PathBuf) -> khora_io::asset::FileLoader = khora_io::asset::FileLoader::new;
    let _ = khora_io::asset::FileLoader::root;
    is_asset_io::<khora_io::asset::FileLoader>();
    is_asset_writer::<khora_io::asset::FileLoader>();
    let _ = type_name::<khora_io::asset::decoders::mesh::GltfDecoder>();
    let _ = type_name::<khora_io::asset::decoders::GltfDecoder>();
    let _ = type_name::<khora_io::asset::mesh::GltfDecoder>();
    let _ = type_name::<khora_io::asset::GltfDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::mesh::GltfDecoder>,
        PhantomData::<khora_io::asset::GltfDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::decoders::GltfDecoder>,
        PhantomData::<khora_io::asset::GltfDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::mesh::GltfDecoder>,
        PhantomData::<khora_io::asset::GltfDecoder>,
    );
    let _ = khora_io::asset::GltfDecoder::new;
    is_clone::<khora_io::asset::GltfDecoder>();
    is_asset_decoder_mesh::<khora_io::asset::GltfDecoder>();
    let _ = type_name::<khora_io::asset::IndexBuilder<'static>>();
    let _ = khora_io::asset::IndexBuilder::new;
    let _ = khora_io::asset::IndexBuilder::with_registry;
    let _ = khora_io::asset::IndexBuilder::build_metadata;
    let _ = khora_io::asset::IndexBuilder::build_index_bytes;
    let _ = type_name::<khora_io::asset::ManifestEntry>();
    let _ = manifest_entry_fields as fn(&khora_io::asset::ManifestEntry);
    is_debug::<khora_io::asset::ManifestEntry>();
    is_clone::<khora_io::asset::ManifestEntry>();
    is_serialize::<khora_io::asset::ManifestEntry>();
    is_deserialize_owned::<khora_io::asset::ManifestEntry>();
    let _ = type_name::<khora_io::asset::decoders::mesh::MeshDispatcher>();
    let _ = type_name::<khora_io::asset::decoders::MeshDispatcher>();
    let _ = type_name::<khora_io::asset::mesh::MeshDispatcher>();
    let _ = type_name::<khora_io::asset::MeshDispatcher>();
    same_type(
        PhantomData::<khora_io::asset::decoders::mesh::MeshDispatcher>,
        PhantomData::<khora_io::asset::MeshDispatcher>,
    );
    same_type(
        PhantomData::<khora_io::asset::decoders::MeshDispatcher>,
        PhantomData::<khora_io::asset::MeshDispatcher>,
    );
    same_type(
        PhantomData::<khora_io::asset::mesh::MeshDispatcher>,
        PhantomData::<khora_io::asset::MeshDispatcher>,
    );
    let _ = khora_io::asset::MeshDispatcher::new;
    is_clone::<khora_io::asset::MeshDispatcher>();
    is_default::<khora_io::asset::MeshDispatcher>();
    is_asset_decoder_mesh::<khora_io::asset::MeshDispatcher>();
    let _ = type_name::<khora_io::asset::decoders::mesh::ObjDecoder>();
    let _ = type_name::<khora_io::asset::decoders::ObjDecoder>();
    let _ = type_name::<khora_io::asset::mesh::ObjDecoder>();
    let _ = type_name::<khora_io::asset::ObjDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::mesh::ObjDecoder>,
        PhantomData::<khora_io::asset::ObjDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::decoders::ObjDecoder>,
        PhantomData::<khora_io::asset::ObjDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::mesh::ObjDecoder>,
        PhantomData::<khora_io::asset::ObjDecoder>,
    );
    is_clone::<khora_io::asset::ObjDecoder>();
    is_default::<khora_io::asset::ObjDecoder>();
    is_asset_decoder_mesh::<khora_io::asset::ObjDecoder>();
    let _ = khora_io::asset::PACK_FLAG_LZ4;
    let _ = khora_io::asset::PACK_FLAG_MANIFEST;
    let _ = khora_io::asset::PACK_FORMAT_VERSION;
    let _ = khora_io::asset::PACK_HEADER_SIZE;
    let _ = khora_io::asset::PACK_MAGIC;
    let _ = type_name::<khora_io::asset::PackBuilder<'static>>();
    let _ = khora_io::asset::PackBuilder::new;
    let _ = khora_io::asset::PackBuilder::with_progress;
    let _ = khora_io::asset::PackBuilder::with_compression;
    let _ = khora_io::asset::PackBuilder::with_manifest;
    let _ = khora_io::asset::PackBuilder::build;
    let _ = type_name::<khora_io::asset::PackHeader>();
    let _ = pack_header_fields as fn(&khora_io::asset::PackHeader);
    let _ = khora_io::asset::PackHeader::to_bytes;
    let _ = khora_io::asset::PackHeader::v2;
    is_debug::<khora_io::asset::PackHeader>();
    is_clone::<khora_io::asset::PackHeader>();
    is_copy::<khora_io::asset::PackHeader>();
    let _ = type_name::<khora_io::asset::PackLoader>();
    let _ = khora_io::asset::PackLoader::new;
    let _ = khora_io::asset::PackLoader::header;
    #[allow(clippy::type_complexity)]
    let _: fn(&mut Vec<u8>, u32, u32) -> anyhow::Result<()> =
        khora_io::asset::PackLoader::write_header;
    is_debug::<khora_io::asset::PackLoader>();
    is_asset_io::<khora_io::asset::PackLoader>();
    let _ = type_name::<khora_io::asset::PackManifest>();
    let _ = khora_io::asset::PackManifest::new;
    let _ = khora_io::asset::PackManifest::insert;
    let _ = khora_io::asset::PackManifest::encode;
    let _ = khora_io::asset::PackManifest::decode;
    let _ = khora_io::asset::PackManifest::is_empty;
    let _ = khora_io::asset::PackManifest::len;
    let _ = khora_io::asset::PackManifest::get;
    let _ = khora_io::asset::PackManifest::verify;
    is_debug::<khora_io::asset::PackManifest>();
    is_default::<khora_io::asset::PackManifest>();
    is_clone::<khora_io::asset::PackManifest>();
    let _ = type_name::<khora_io::asset::PackOutput>();
    let _ = pack_output_fields as fn(&khora_io::asset::PackOutput);
    let _ = khora_io::asset::PackOutput::pack_bytes;
    is_debug::<khora_io::asset::PackOutput>();
    is_clone::<khora_io::asset::PackOutput>();
    let _ = type_name::<khora_io::asset::PackProgress>();
    let _ = pack_progress_variants as fn(&khora_io::asset::PackProgress);
    is_debug::<khora_io::asset::PackProgress>();
    is_clone::<khora_io::asset::PackProgress>();
    let _ = khora_io::asset::REGISTRY_DIR;
    let _ = khora_io::asset::REGISTRY_FILE;
    let _ = type_name::<khora_io::asset::decoders::audio::SymphoniaDecoder>();
    let _ = type_name::<khora_io::asset::decoders::SymphoniaDecoder>();
    let _ = type_name::<khora_io::asset::audio::SymphoniaDecoder>();
    let _ = type_name::<khora_io::asset::SymphoniaDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::audio::SymphoniaDecoder>,
        PhantomData::<khora_io::asset::SymphoniaDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::decoders::SymphoniaDecoder>,
        PhantomData::<khora_io::asset::SymphoniaDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::audio::SymphoniaDecoder>,
        PhantomData::<khora_io::asset::SymphoniaDecoder>,
    );
    let _ = khora_io::asset::SymphoniaDecoder::new;
    is_default::<khora_io::asset::SymphoniaDecoder>();
    is_asset_decoder_sound_data::<khora_io::asset::SymphoniaDecoder>();
    let _ = type_name::<khora_io::asset::decoders::audio::WavDecoder>();
    let _ = type_name::<khora_io::asset::decoders::WavDecoder>();
    let _ = type_name::<khora_io::asset::audio::WavDecoder>();
    let _ = type_name::<khora_io::asset::WavDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::audio::WavDecoder>,
        PhantomData::<khora_io::asset::WavDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::decoders::WavDecoder>,
        PhantomData::<khora_io::asset::WavDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::audio::WavDecoder>,
        PhantomData::<khora_io::asset::WavDecoder>,
    );
    let _ = khora_io::asset::WavDecoder::new;
    is_default::<khora_io::asset::WavDecoder>();
    is_asset_decoder_sound_data::<khora_io::asset::WavDecoder>();
    let _ = khora_io::asset::asset_type_for_extension;
    let _ = type_name::<khora_io::asset::decoders::font::FontDecoder>();
    let _ = type_name::<khora_io::asset::decoders::FontDecoder>();
    let _ = type_name::<khora_io::asset::font::FontDecoder>();
    let _ = type_name::<khora_io::asset::FontDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::FontDecoder>,
        PhantomData::<khora_io::asset::decoders::font::FontDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::font::FontDecoder>,
        PhantomData::<khora_io::asset::decoders::font::FontDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::FontDecoder>,
        PhantomData::<khora_io::asset::decoders::font::FontDecoder>,
    );
    is_clone::<khora_io::asset::decoders::font::FontDecoder>();
    is_default::<khora_io::asset::decoders::font::FontDecoder>();
    is_asset_decoder_font::<khora_io::asset::decoders::font::FontDecoder>();
    let _ = type_name::<khora_io::asset::decoders::material::MaterialDecoder>();
    let _ = type_name::<khora_io::asset::decoders::MaterialDecoder>();
    let _ = type_name::<khora_io::asset::material::MaterialDecoder>();
    let _ = type_name::<khora_io::asset::MaterialDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::MaterialDecoder>,
        PhantomData::<khora_io::asset::decoders::material::MaterialDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::material::MaterialDecoder>,
        PhantomData::<khora_io::asset::decoders::material::MaterialDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::MaterialDecoder>,
        PhantomData::<khora_io::asset::decoders::material::MaterialDecoder>,
    );
    is_clone::<khora_io::asset::decoders::material::MaterialDecoder>();
    is_default::<khora_io::asset::decoders::material::MaterialDecoder>();
    is_asset_decoder_box_dyn_material::<khora_io::asset::decoders::material::MaterialDecoder>();
    let _ = khora_io::asset::decoders::material::decode_material;
    let _ = khora_io::asset::decoders::decode_material;
    let _ = khora_io::asset::material::decode_material;
    let _ = khora_io::asset::decode_material;
    same_item(
        &khora_io::asset::decoders::decode_material,
        &khora_io::asset::decoders::material::decode_material,
    );
    same_item(
        &khora_io::asset::material::decode_material,
        &khora_io::asset::decoders::material::decode_material,
    );
    same_item(
        &khora_io::asset::decode_material,
        &khora_io::asset::decoders::material::decode_material,
    );
    let _ = type_name::<khora_io::asset::decoders::mesh::FileSystemResolver>();
    let _ = type_name::<khora_io::asset::decoders::FileSystemResolver>();
    let _ = type_name::<khora_io::asset::mesh::FileSystemResolver>();
    let _ = type_name::<khora_io::asset::FileSystemResolver>();
    same_type(
        PhantomData::<khora_io::asset::decoders::FileSystemResolver>,
        PhantomData::<khora_io::asset::decoders::mesh::FileSystemResolver>,
    );
    same_type(
        PhantomData::<khora_io::asset::mesh::FileSystemResolver>,
        PhantomData::<khora_io::asset::decoders::mesh::FileSystemResolver>,
    );
    same_type(
        PhantomData::<khora_io::asset::FileSystemResolver>,
        PhantomData::<khora_io::asset::decoders::mesh::FileSystemResolver>,
    );
    let _: fn(&'static std::path::Path) -> khora_io::asset::decoders::mesh::FileSystemResolver =
        khora_io::asset::decoders::mesh::FileSystemResolver::new;
    is_gltf_resource_resolver::<khora_io::asset::decoders::mesh::FileSystemResolver>();
    // trait `khora_io::asset::decoders::mesh::GltfResourceResolver`: see `gltf_resource_resolver_trait_items`
    // trait `khora_io::asset::decoders::GltfResourceResolver`: see `gltf_resource_resolver_trait_items`
    // trait `khora_io::asset::mesh::GltfResourceResolver`: see `gltf_resource_resolver_trait_items`
    // trait `khora_io::asset::GltfResourceResolver`: see `gltf_resource_resolver_trait_items`
    let _ = type_name::<khora_io::asset::decoders::mesh::NoOpResourceResolver>();
    let _ = type_name::<khora_io::asset::decoders::NoOpResourceResolver>();
    let _ = type_name::<khora_io::asset::mesh::NoOpResourceResolver>();
    let _ = type_name::<khora_io::asset::NoOpResourceResolver>();
    same_type(
        PhantomData::<khora_io::asset::decoders::NoOpResourceResolver>,
        PhantomData::<khora_io::asset::decoders::mesh::NoOpResourceResolver>,
    );
    same_type(
        PhantomData::<khora_io::asset::mesh::NoOpResourceResolver>,
        PhantomData::<khora_io::asset::decoders::mesh::NoOpResourceResolver>,
    );
    same_type(
        PhantomData::<khora_io::asset::NoOpResourceResolver>,
        PhantomData::<khora_io::asset::decoders::mesh::NoOpResourceResolver>,
    );
    is_gltf_resource_resolver::<khora_io::asset::decoders::mesh::NoOpResourceResolver>();
    let _ = type_name::<khora_io::asset::decoders::script::ScriptDecoder>();
    let _ = type_name::<khora_io::asset::decoders::ScriptDecoder>();
    let _ = type_name::<khora_io::asset::script::ScriptDecoder>();
    let _ = type_name::<khora_io::asset::ScriptDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::ScriptDecoder>,
        PhantomData::<khora_io::asset::decoders::script::ScriptDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::script::ScriptDecoder>,
        PhantomData::<khora_io::asset::decoders::script::ScriptDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::ScriptDecoder>,
        PhantomData::<khora_io::asset::decoders::script::ScriptDecoder>,
    );
    is_clone::<khora_io::asset::decoders::script::ScriptDecoder>();
    is_default::<khora_io::asset::decoders::script::ScriptDecoder>();
    is_asset_decoder_script_module::<khora_io::asset::decoders::script::ScriptDecoder>();
    let _ = type_name::<khora_io::asset::decoders::shader::ShaderDecoder>();
    let _ = type_name::<khora_io::asset::decoders::ShaderDecoder>();
    let _ = type_name::<khora_io::asset::shader::ShaderDecoder>();
    let _ = type_name::<khora_io::asset::ShaderDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::ShaderDecoder>,
        PhantomData::<khora_io::asset::decoders::shader::ShaderDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::shader::ShaderDecoder>,
        PhantomData::<khora_io::asset::decoders::shader::ShaderDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::ShaderDecoder>,
        PhantomData::<khora_io::asset::decoders::shader::ShaderDecoder>,
    );
    is_clone::<khora_io::asset::decoders::shader::ShaderDecoder>();
    is_default::<khora_io::asset::decoders::shader::ShaderDecoder>();
    is_asset_decoder_cpu_shader_source::<khora_io::asset::decoders::shader::ShaderDecoder>();
    let _ = type_name::<khora_io::asset::decoders::texture::TextureDecoder>();
    let _ = type_name::<khora_io::asset::decoders::TextureDecoder>();
    let _ = type_name::<khora_io::asset::texture::TextureDecoder>();
    let _ = type_name::<khora_io::asset::TextureDecoder>();
    same_type(
        PhantomData::<khora_io::asset::decoders::TextureDecoder>,
        PhantomData::<khora_io::asset::decoders::texture::TextureDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::texture::TextureDecoder>,
        PhantomData::<khora_io::asset::decoders::texture::TextureDecoder>,
    );
    same_type(
        PhantomData::<khora_io::asset::TextureDecoder>,
        PhantomData::<khora_io::asset::decoders::texture::TextureDecoder>,
    );
    is_clone::<khora_io::asset::decoders::texture::TextureDecoder>();
    is_default::<khora_io::asset::decoders::texture::TextureDecoder>();
    is_asset_decoder_cpu_texture::<khora_io::asset::decoders::texture::TextureDecoder>();
    let _ = khora_io::asset::extract_dependencies;
    let _ = khora_io::asset::should_skip_file;
    let _ = khora_io::asset::type_has_dependency_extractor;
}

#[test]
fn module_script_compile_paths_still_resolve() {
    let _ = type_name::<khora_io::script_compile::DiskLoader>();
    let _: fn(std::path::PathBuf) -> khora_io::script_compile::DiskLoader =
        khora_io::script_compile::DiskLoader::new;
    is_debug::<khora_io::script_compile::DiskLoader>();
    is_clone::<khora_io::script_compile::DiskLoader>();
    is_source_loader::<khora_io::script_compile::DiskLoader>();
}

#[test]
fn module_script_hot_reload_paths_still_resolve() {
    let _ = type_name::<khora_io::script_hot_reload::PendingReloads>();
    let _ = khora_io::script_hot_reload::load_all;
    let _ = khora_io::script_hot_reload::reload_channel;
}

#[test]
fn module_script_mirror_paths_still_resolve() {
    let _ = khora_io::script_mirror::MIRROR_MODULE;
    let _ = type_name::<khora_io::script_mirror::PreludeLoader<khora_script::MemoryLoader>>();
    let _ = <khora_io::script_mirror::PreludeLoader<khora_script::MemoryLoader>>::new;
    let _ = <khora_io::script_mirror::PreludeLoader<khora_script::MemoryLoader>>::inner;
    is_debug::<khora_io::script_mirror::PreludeLoader<khora_script::MemoryLoader>>();
    is_clone::<khora_io::script_mirror::PreludeLoader<khora_script::MemoryLoader>>();
    is_source_loader::<khora_io::script_mirror::PreludeLoader<khora_script::MemoryLoader>>();
    let _ = khora_io::script_mirror::mirror_source;
}

#[test]
fn module_serialization_paths_still_resolve() {
    let _ = khora_io::serialization::CURRENT_SCENE_VERSION;
    let _ = type_name::<khora_io::serialization::SerializationService>();
    let _ = khora_io::serialization::SerializationService::new;
    let _ = khora_io::serialization::SerializationService::save_world;
    let _ = khora_io::serialization::SerializationService::load_world;
    is_default::<khora_io::serialization::SerializationService>();
    let _ = type_name::<khora_io::serialization::SerializationServiceError>();
    let _ = serialization_service_error_variants
        as fn(&khora_io::serialization::SerializationServiceError);
    is_debug::<khora_io::serialization::SerializationServiceError>();
}

#[test]
fn module_shader_hot_reload_paths_still_resolve() {
    let _ = khora_io::shader_hot_reload::logical_name_for_shader_path;
}

#[test]
fn module_vfs_paths_still_resolve() {
    let _ = type_name::<khora_io::vfs::VirtualFileSystem>();
    let _ = khora_io::vfs::VirtualFileSystem::new;
    let _ = khora_io::vfs::VirtualFileSystem::get_metadata;
    let _ = khora_io::vfs::VirtualFileSystem::iter_all;
    let _ = khora_io::vfs::VirtualFileSystem::asset_count;
    is_debug::<khora_io::vfs::VirtualFileSystem>();
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `hub/`,
// `examples/`, `xtask/`; brace imports expanded). The trailing comment names
// the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_io::asset::decoders::audio::SymphoniaDecoder as _; // khora-sdk
    use khora_io::asset::decoders::TextureDecoder as _; // khora-agents
    use khora_io::asset::AssetChangeEvent as _; // khora-sdk
    use khora_io::asset::AssetChangeKind as _; // khora-sdk
    use khora_io::asset::AssetDecoder as _; // khora-agents
    use khora_io::asset::AssetIdRegistry as _; // khora-sdk
    use khora_io::asset::AssetIo as _; // khora-sdk
    use khora_io::asset::AssetService as _; // khora-agents, khora-sdk
    use khora_io::asset::AssetWatcher as _; // khora-sdk
    use khora_io::asset::AssetWriter as _; // khora-sdk
    use khora_io::asset::FileLoader as _; // khora-sdk
    use khora_io::asset::FileSystemResolver as _; // khora-sdk
    use khora_io::asset::IndexBuilder as _; // khora-sdk
    use khora_io::asset::MeshDispatcher as _; // khora-sdk
    use khora_io::asset::PackBuilder as _; // khora-sdk
    use khora_io::asset::PackHeader as _; // khora-agents, khora-sdk
    use khora_io::asset::PackLoader as _; // khora-agents, khora-sdk, xtask
    use khora_io::asset::PackManifest as _; // khora-sdk
    use khora_io::asset::PackOutput as _; // khora-sdk
    use khora_io::asset::PackProgress as _; // khora-sdk
    use khora_io::asset::SymphoniaDecoder as _; // khora-sdk
    use khora_io::asset::PACK_FORMAT_VERSION as _; // khora-sdk
    use khora_io::asset::PACK_HEADER_SIZE as _; // khora-sdk
    use khora_io::asset::PACK_MAGIC as _; // khora-sdk
    use khora_io::script_compile::DiskLoader as _; // khora-agents
    use khora_io::script_hot_reload::load_all as _; // khora-sdk
    use khora_io::script_hot_reload::reload_channel as _; // khora-agents, khora-sdk
    use khora_io::script_hot_reload::PendingReloads as _; // khora-sdk
    use khora_io::serialization::SerializationService as _; // khora-sdk
}
