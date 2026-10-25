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

//! A text's font handle follows its font over its life, texts sharing a font
//! share one load, a missing service or store never panics, and an asset that
//! fails to load is not read again on every frame.

use super::{resolve_ui_fonts, resolve_ui_images};
use crate::asset::{AssetIo, AssetService, FileLoader, IndexBuilder};
use khora_core::asset::font::Font;
use khora_core::asset::{AssetSource, AssetUUID};
use khora_core::math::Vec2;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::Runtime;
use khora_data::ecs::{HandleComponent, World};
use khora_data::ui::components::{UiImage, UiText, UiTransform};
use khora_data::AssetStore;
use khora_telemetry::MetricsRegistry;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// Reads files through a [`FileLoader`] and counts every read.
struct CountingIo {
    inner: FileLoader,
    reads: Arc<AtomicUsize>,
}

impl AssetIo for CountingIo {
    fn load_bytes(&mut self, source: &AssetSource) -> anyhow::Result<Vec<u8>> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.inner.load_bytes(source)
    }
}

/// A project assets dir on disk, indexed and served by a real `AssetService`
/// whose reads are counted, wired into a runtime with a shared `AssetStore`.
struct Project {
    dir: TempDir,
    service: Arc<Mutex<AssetService>>,
    store: AssetStore,
    runtime: Runtime,
    reads: Arc<AtomicUsize>,
}

impl Project {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        for (rel, bytes) in files {
            let path = dir.path().join(rel);
            fs::create_dir_all(path.parent().expect("a parent dir")).expect("create dirs");
            fs::write(path, bytes).expect("write asset file");
        }
        let index_bytes = IndexBuilder::new(dir.path())
            .build_index_bytes()
            .expect("index the project");
        let reads = Arc::new(AtomicUsize::new(0));
        let io = CountingIo {
            inner: FileLoader::new(dir.path()),
            reads: reads.clone(),
        };
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(io),
            Arc::new(MetricsRegistry::new()),
            None,
        )
        .expect("an asset service over the project");
        service.register_inventory_decoders();
        let service = Arc::new(Mutex::new(service));

        let store = AssetStore::new();
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime.services.insert(service.clone());
        Self {
            dir,
            service,
            store,
            runtime,
            reads,
        }
    }

    fn path(&self, rel: &str) -> std::path::PathBuf {
        self.dir.path().join(rel)
    }

    /// One frame of the resolver's UI passes.
    fn resolve(&self, world: &mut World) {
        resolve_ui_fonts(world, &self.runtime);
        resolve_ui_images(world, &self.runtime);
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    fn load_count(&self) -> usize {
        self.service.lock().expect("service lock").load_count()
    }
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

/// The font id the entity's handle names, if it has one.
fn handle_of(world: &World, entity: khora_core::ecs::entity::EntityId) -> Option<AssetUUID> {
    world.get::<HandleComponent<Font>>(entity).map(|h| h.uuid)
}

/// Sets the font the entity's text names.
fn rename_font(world: &mut World, entity: khora_core::ecs::entity::EntityId, font: AssetUUID) {
    let mut renamed = world.get::<UiText>(entity).expect("a text").clone();
    renamed.font = font;
    assert!(world.set_component(entity, renamed), "the text is renamed");
}

#[test]
fn a_text_font_handle_follows_built_in_then_two_assets_then_built_in_again() {
    let (title, body) = ("fonts/title.ttf", "fonts/body.ttf");
    let (title_id, body_id) = (AssetUUID::new_v5(title), AssetUUID::new_v5(body));
    let project = Project::new(&[(title, b"title bytes"), (body, b"body bytes")]);
    let mut world = World::new();
    let entity = world.spawn((transform(), text(Font::BUILT_IN)));

    project.resolve(&mut world);
    assert_eq!(
        handle_of(&world, entity),
        None,
        "the built-in face has no handle"
    );

    rename_font(&mut world, entity, title_id);
    project.resolve(&mut world);
    assert_eq!(handle_of(&world, entity), Some(title_id));
    assert_eq!(
        world
            .get::<HandleComponent<Font>>(entity)
            .map(|h| h.handle.data.clone()),
        Some(b"title bytes".to_vec())
    );

    rename_font(&mut world, entity, body_id);
    project.resolve(&mut world);
    assert_eq!(handle_of(&world, entity), Some(body_id));
    assert_eq!(
        world
            .get::<HandleComponent<Font>>(entity)
            .map(|h| h.handle.data.clone()),
        Some(b"body bytes".to_vec())
    );

    rename_font(&mut world, entity, Font::BUILT_IN);
    project.resolve(&mut world);
    assert_eq!(handle_of(&world, entity), None, "back to the built-in face");
    assert_eq!(
        world.get::<UiText>(entity).map(|t| t.content.clone()),
        Some("Hello".to_owned()),
        "the text itself is untouched"
    );
}

#[test]
fn texts_naming_one_font_share_one_load_and_one_font() {
    let path = "fonts/body.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, b"body bytes")]);
    let mut world = World::new();
    let first = world.spawn((transform(), text(font)));
    let second = world.spawn((transform(), text(font)));

    project.resolve(&mut world);

    assert_eq!(project.load_count(), 1, "the font is decoded once");
    assert_eq!(project.reads(), 1, "the font file is read once");
    let a = world
        .get::<HandleComponent<Font>>(first)
        .expect("first resolved");
    let b = world
        .get::<HandleComponent<Font>>(second)
        .expect("second resolved");
    assert!(
        std::ptr::eq(&*a.handle, &*b.handle),
        "both texts hold the same loaded font"
    );
}

#[test]
fn without_an_asset_service_or_store_the_resolver_leaves_the_ui_as_it_is() {
    let font = AssetUUID::new_v5("fonts/body.ttf");
    let mut world = World::new();
    let entity = world.spawn((transform(), text(font)));
    world.spawn((
        transform(),
        UiImage {
            texture: AssetUUID::new_v5("ui/icon.png"),
        },
    ));
    let runtime = Runtime::default();

    resolve_ui_fonts(&mut world, &runtime);
    resolve_ui_images(&world, &runtime);

    assert_eq!(handle_of(&world, entity), None, "no font without a service");
}

#[test]
fn a_ui_image_that_fails_to_decode_is_not_read_again_on_every_frame() {
    let path = "ui/broken.png";
    let texture = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, b"not an image")]);
    let mut world = World::new();
    world.spawn((transform(), UiImage { texture }));

    for _ in 0..10 {
        project.resolve(&mut world);
    }

    assert!(
        project
            .store
            .store::<CpuTexture>()
            .read()
            .expect("store lock")
            .get(&texture)
            .is_none(),
        "undecodable bytes give no texture"
    );
    assert_eq!(
        project.reads(),
        1,
        "a failed image is read once, not once a frame"
    );
}

#[test]
fn a_text_font_that_fails_to_load_is_not_read_again_on_every_frame() {
    let path = "fonts/gone.ttf";
    let font = AssetUUID::new_v5(path);
    let project = Project::new(&[(path, b"font bytes")]);
    // The font is indexed, then its file disappears.
    fs::remove_file(project.path(path)).expect("remove the font file");
    let mut world = World::new();
    let entity = world.spawn((transform(), text(font)));

    for _ in 0..10 {
        project.resolve(&mut world);
    }

    assert_eq!(
        handle_of(&world, entity),
        None,
        "a missing font gives no handle"
    );
    assert_eq!(
        project.reads(),
        1,
        "a failed font is read once, not once a frame"
    );
}
