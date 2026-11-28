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

//! UI assets: the resolver gives each `UiText` the font it names and
//! loads every `UiImage` texture into the shared `Assets<CpuTexture>`.

use std::fs;
use std::path::Path;

use khora_core::asset::font::Font;
use khora_core::math::Vec2;
use khora_core::renderer::api::resource::CpuTexture;
use khora_data::ui::components::{UiImage, UiText, UiTransform};
use khora_telemetry::MetricsRegistry;
use tempfile::TempDir;

use super::*;
use crate::asset::{FileLoader, IndexBuilder};

/// A project assets dir on disk, served by a real `AssetService` in a runtime.
///
/// Its decoders are the registered ones; the runtime holds a shared
/// `AssetStore`.
struct Project {
    _dir: TempDir,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    runtime: Runtime,
}

impl Project {
    fn new(files: &[(&str, Vec<u8>)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (rel, bytes) in files {
            write_file(dir.path(), rel, bytes);
        }
        let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            dir.path(),
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(dir.path())),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .unwrap();
        service.register_inventory_decoders();
        let service = Arc::new(Mutex::new(service));

        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.resources.insert(khora_core::lane::Reserve::new());
        runtime.services.insert(service.clone());
        Self {
            _dir: dir,
            service,
            store,
            runtime,
        }
    }

    /// Frames of the resolver until what it asked for is decoded off
    /// the frame and taken in at a boundary.
    fn resolve(&self, world: &mut World) {
        resolve_settled(world, &self.runtime);
    }

    fn font(&self, uuid: &AssetUUID) -> Option<AssetHandle<Font>> {
        self.store
            .store::<Font>()
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
    }

    fn texture(&self, uuid: &AssetUUID) -> Option<AssetHandle<CpuTexture>> {
        self.store
            .store::<CpuTexture>()
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
    }

    fn load_count(&self) -> usize {
        self.service.lock().unwrap().load_count()
    }
}

fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A real PNG of `width` x `height`, encoded in memory.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png");
    bytes
}

fn transform() -> UiTransform {
    UiTransform {
        pos: Vec2::new(0.0, 0.0),
        size: Vec2::new(100.0, 20.0),
        z_index: 0,
    }
}

fn text(font: AssetUUID) -> UiText {
    UiText {
        content: "Hello".to_owned(),
        font,
        ..Default::default()
    }
}

fn font_handle(uuid: AssetUUID, data: &[u8]) -> HandleComponent<Font> {
    HandleComponent {
        handle: AssetHandle::new(Font {
            name: "Stale".to_owned(),
            data: data.to_vec(),
        }),
        uuid,
    }
}

/// The bytes of a font file; the font decoder keeps them as they are.
// A TrueType file starts with these bytes: the font decoder reads only
// what does.
const FONT_BYTES: &[u8] = b"\0\x01\0\0font file bytes";

#[test]
fn a_text_naming_a_font_asset_gets_its_handle_and_the_font_lands_in_the_store() {
    let path = "fonts/body.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, FONT_BYTES.to_vec())]);
    let mut world = World::new();
    let entity = world.spawn((transform(), text(font)));

    project.resolve(&mut world);

    let resolved = world
        .get::<HandleComponent<Font>>(entity)
        .expect("the text's font is resolved onto the entity");
    assert_eq!(resolved.uuid, font);
    assert_eq!(resolved.handle.data, FONT_BYTES);
    let stored = project
        .font(&font)
        .expect("the font is loaded into Assets<Font>");
    assert_eq!(stored.data, FONT_BYTES);
}

#[test]
fn a_font_handle_naming_another_font_is_replaced_by_the_named_one() {
    let path = "fonts/title.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, FONT_BYTES.to_vec())]);
    let mut world = World::new();
    let entity = world.spawn((
        transform(),
        text(font),
        font_handle(AssetUUID::new(), b"former font"),
    ));

    project.resolve(&mut world);

    let resolved = world
        .get::<HandleComponent<Font>>(entity)
        .expect("the entity keeps a font handle");
    assert_eq!(resolved.uuid, font, "the handle follows the text's font");
    assert_eq!(resolved.handle.data, FONT_BYTES);
}

#[test]
fn a_resolved_font_is_not_loaded_again() {
    let path = "fonts/body.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, FONT_BYTES.to_vec())]);
    let mut world = World::new();
    world.spawn((transform(), text(font)));

    project.resolve(&mut world);
    let loads = project.load_count();
    project.resolve(&mut world);

    assert_eq!(loads, 1, "the font is loaded once");
    assert_eq!(project.load_count(), loads, "steady state loads nothing");
}

#[test]
fn a_built_in_font_loads_nothing_and_drops_a_stale_handle() {
    let project = Project::new(&[]);
    let mut world = World::new();
    let entity = world.spawn((
        transform(),
        text(Font::BUILT_IN),
        font_handle(AssetUUID::new(), b"former font"),
    ));

    project.resolve(&mut world);

    assert_eq!(project.load_count(), 0, "the built-in face is no asset");
    assert!(
        world.get::<HandleComponent<Font>>(entity).is_none(),
        "a text in the built-in font holds no font handle"
    );
}

#[test]
fn a_ui_image_texture_is_loaded_into_the_store() {
    let path = "ui/icon.png";
    let texture = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, png(3, 2))]);
    let mut world = World::new();
    world.spawn((transform(), UiImage { texture }));

    project.resolve(&mut world);

    let stored = project
        .texture(&texture)
        .expect("the image is loaded into Assets<CpuTexture>");
    assert_eq!((stored.size.width, stored.size.height), (3, 2));
}

#[test]
fn a_ui_image_texture_already_in_the_store_is_not_reloaded() {
    let path = "ui/icon.png";
    let texture = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, png(3, 2))]);
    let mut world = World::new();
    world.spawn((transform(), UiImage { texture }));

    project.resolve(&mut world);
    let loads = project.load_count();
    project.resolve(&mut world);

    assert_eq!(loads, 1, "the image is loaded once");
    assert_eq!(project.load_count(), loads, "steady state loads nothing");
}
