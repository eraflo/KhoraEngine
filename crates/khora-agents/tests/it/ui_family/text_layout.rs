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

//! Texts laid out by the real shaper, through the `PreExtract` phase the
//! engine runs before the flows project the UI.

use std::sync::Arc;

use khora_control::substrate::run_data_systems;
use khora_core::asset::font::Font;
use khora_core::lane::OutputDeck;
use khora_core::ui::text::TextShaper;
use khora_core::Runtime;
use khora_data::ecs::{TickPhase, World};
use khora_data::ui::{UiText, UiTextLayout, UiTransform};
use khora_infra::ParleyShaper;

/// **"Hello" in the built-in font is five glyphs on one line.** The
/// `PreExtract` phase, with Parley as the runtime's shaper, gives the text a
/// layout without any font asset: one glyph per letter, each pen right of the
/// one before.
#[test]
fn hello_in_the_built_in_font_is_laid_out_in_five_glyphs() {
    let mut world = World::new();
    let entity = world.spawn((
        UiTransform::default(),
        UiText {
            content: "Hello".to_owned(),
            font: Font::BUILT_IN,
            size: 16.0,
            ..Default::default()
        },
    ));
    let mut runtime = Runtime::new();
    let shaper: Arc<dyn TextShaper> = Arc::new(ParleyShaper::new());
    runtime.backends.insert(shaper);

    run_data_systems(
        &mut world,
        &runtime,
        &mut OutputDeck::new(),
        TickPhase::PreExtract,
    );

    let layout = world
        .get::<UiTextLayout>(entity)
        .expect("the text is laid out in PreExtract");
    assert_eq!(layout.font, Font::BUILT_IN);
    assert_eq!(layout.size_px, 16.0);
    assert_eq!(layout.lines, 1);
    assert_eq!(layout.glyphs.len(), 5, "{:?}", layout.glyphs);
    assert!(
        layout.glyphs.windows(2).all(|pair| pair[1].x > pair[0].x),
        "the pen advances: {:?}",
        layout.glyphs
    );
    assert!(
        layout.size.x > 0.0 && layout.size.y > 0.0,
        "{:?}",
        layout.size
    );
}

/// The real Inter file, the font the tests register under their own ids.
const INTER: &[u8] = include_bytes!("../../../../khora-infra/assets/fonts/InterVariable.ttf");

/// A font asset the tests name.
const BODY: khora_core::asset::AssetUUID =
    khora_core::asset::AssetUUID::from_bytes(*b"khora:test:body!");

/// Parley, with every registration it is asked for recorded as the length of
/// the data it was handed.
struct RecordingShaper {
    inner: ParleyShaper,
    registered: std::sync::Mutex<Vec<(khora_core::asset::AssetUUID, usize)>>,
}

impl TextShaper for RecordingShaper {
    fn register_font(
        &self,
        id: khora_core::asset::AssetUUID,
        data: Arc<[u8]>,
    ) -> Result<khora_core::ui::text::FontFamily, khora_core::ui::text::FontError> {
        self.registered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, data.len()));
        self.inner.register_font(id, data)
    }

    fn shape(
        &self,
        request: &khora_core::ui::text::ShapeRequest<'_>,
    ) -> Result<khora_core::ui::text::ShapedText, khora_core::ui::text::ShapeError> {
        self.inner.shape(request)
    }
}

fn font_handle(data: &[u8]) -> khora_data::ecs::HandleComponent<Font> {
    khora_data::ecs::HandleComponent {
        handle: khora_core::asset::AssetHandle::new(Font {
            name: "Body".to_owned(),
            data: data.to_vec(),
        }),
        uuid: BODY,
    }
}

fn pre_extract(world: &mut World, runtime: &Runtime) {
    run_data_systems(
        world,
        runtime,
        &mut OutputDeck::new(),
        TickPhase::PreExtract,
    );
}

/// **A text whose font was refused stays without a layout when edited.** A
/// reload that hands the shaper data it cannot read takes the text's layout
/// away; editing the text afterwards must not bring back a layout shaped in
/// the font's previous data, which the entity no longer holds. Parley keeps a
/// font's last good registration when a new one is refused, so a reshape
/// that does not know of the refusal finds the old font still there.
#[test]
fn a_text_whose_font_was_refused_stays_without_a_layout_when_edited() {
    let mut world = World::new();
    let entity = world.spawn((
        UiTransform::default(),
        UiText {
            content: "Readable".to_owned(),
            font: BODY,
            size: 16.0,
            ..Default::default()
        },
        font_handle(INTER),
    ));
    let mut runtime = Runtime::new();
    let shaper: Arc<dyn TextShaper> = Arc::new(ParleyShaper::new());
    runtime.backends.insert(shaper);
    pre_extract(&mut world, &runtime);
    assert!(world.get::<UiTextLayout>(entity).is_some(), "laid out");

    assert!(world.set_component(entity, font_handle(b"not a font")));
    pre_extract(&mut world, &runtime);
    assert!(
        world.get::<UiTextLayout>(entity).is_none(),
        "the refused font takes the layout away"
    );

    world.get_mut::<UiText>(entity).expect("a text").content = "Edited".to_owned();
    pre_extract(&mut world, &runtime);

    assert!(
        world.get::<UiTextLayout>(entity).is_none(),
        "the entity still holds the refused font: {:?}",
        world.get::<UiTextLayout>(entity)
    );
}

/// **An edited font file is registered again and its texts reshaped.** The
/// editor's hot reload invalidates the file's asset; the next `PreExtract`
/// (`asset_reload`, `asset_resolver`, `ui_text_layout`) must bring the new
/// file to the text's font handle, so that the shaper is handed it.
#[test]
fn an_edited_font_file_is_registered_again_with_the_shaper() {
    use std::sync::Mutex;

    use khora_data::ecs::HandleComponent;
    use khora_data::AssetStore;
    use khora_io::asset::{AssetIdRegistry, AssetService};
    use khora_telemetry::MetricsRegistry;

    let dir = tempfile::tempdir().expect("a temporary project");
    let root = dir.path().to_path_buf();
    let path = "fonts/body.ttf";
    let file = root.join("assets").join(path);
    std::fs::create_dir_all(file.parent().expect("a parent")).expect("the folder");
    std::fs::write(&file, INTER).expect("the font file");
    let font = khora_core::asset::AssetUUID::new_v5(path);

    let service = AssetService::open_loose_files(
        &root,
        &AssetIdRegistry::load(&root),
        Arc::new(MetricsRegistry::new()),
    )
    .expect("the project opens");
    let service = Arc::new(Mutex::new(service));
    let shaper = Arc::new(RecordingShaper {
        inner: ParleyShaper::new(),
        registered: Mutex::new(Vec::new()),
    });
    let mut runtime = Runtime::new();
    runtime.resources.insert(AssetStore::new());
    runtime.services.insert(service.clone());
    let backend: Arc<dyn TextShaper> = shaper.clone();
    runtime.backends.insert(backend);

    let mut world = World::new();
    let entity = world.spawn((
        UiTransform::default(),
        UiText {
            content: "Hot".to_owned(),
            font,
            size: 16.0,
            ..Default::default()
        },
    ));
    pre_extract(&mut world, &runtime);
    pre_extract(&mut world, &runtime);
    assert_eq!(
        world.get::<UiTextLayout>(entity).expect("laid out").font,
        font
    );

    // The same face, with padding after its tables: a different file that
    // reads as the same font.
    let mut edited = INTER.to_vec();
    edited.extend_from_slice(&[0; 64]);
    std::fs::write(&file, &edited).expect("the edit");
    service.lock().expect("the service").invalidate(&font);
    pre_extract(&mut world, &runtime);
    pre_extract(&mut world, &runtime);

    assert_eq!(
        world
            .get::<HandleComponent<Font>>(entity)
            .expect("the text keeps its font handle")
            .handle
            .data
            .len(),
        edited.len(),
        "the handle holds the edited file"
    );
    assert_eq!(
        shaper
            .registered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last(),
        Some(&(font, edited.len())),
        "the shaper was handed the edited file"
    );
}

/// A loose-file project served into a runtime with Parley as its shaper.
struct FontProject {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    service: Arc<std::sync::Mutex<khora_io::asset::AssetService>>,
    runtime: Runtime,
}

impl FontProject {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let dir = tempfile::tempdir().expect("a temporary project");
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("assets").join("fonts")).expect("the folder");
        for (path, bytes) in files {
            std::fs::write(root.join("assets").join(path), bytes).expect("the file");
        }
        let service = khora_io::asset::AssetService::open_loose_files(
            &root,
            &khora_io::asset::AssetIdRegistry::load(&root),
            Arc::new(khora_telemetry::MetricsRegistry::new()),
        )
        .expect("the project opens");
        let service = Arc::new(std::sync::Mutex::new(service));
        let mut runtime = Runtime::new();
        runtime.resources.insert(khora_data::AssetStore::new());
        runtime.services.insert(service.clone());
        let shaper: Arc<dyn TextShaper> = Arc::new(ParleyShaper::new());
        runtime.backends.insert(shaper);
        Self {
            _dir: dir,
            root,
            service,
            runtime,
        }
    }

    /// Lists the assets folder again and announces `path`'s change, as the
    /// editor's hot reload does when a file appears or goes away.
    fn reindex_and_invalidate(&self, path: &str) {
        let bytes = khora_io::asset::IndexBuilder::new(&self.root.join("assets"))
            .with_registry(&khora_io::asset::AssetIdRegistry::load(&self.root))
            .build_index_bytes()
            .expect("the index");
        let mut service = self.service.lock().expect("the service");
        service.reindex(&bytes).expect("reindexed");
        service.invalidate(&khora_core::asset::AssetUUID::new_v5(path));
    }

    fn text(
        world: &mut World,
        font: khora_core::asset::AssetUUID,
    ) -> khora_core::ecs::entity::EntityId {
        world.spawn((
            UiTransform::default(),
            UiText {
                content: "Hot".to_owned(),
                font,
                size: 16.0,
                ..Default::default()
            },
        ))
    }
}

/// **A deleted font file takes its texts' layouts away.**
#[test]
fn a_deleted_font_file_takes_its_texts_layouts_away() {
    let path = "fonts/body.ttf";
    let project = FontProject::new(&[(path, INTER)]);
    let mut world = World::new();
    let entity = FontProject::text(&mut world, khora_core::asset::AssetUUID::new_v5(path));
    pre_extract(&mut world, &project.runtime);
    assert!(world.get::<UiTextLayout>(entity).is_some(), "laid out");

    std::fs::remove_file(project.root.join("assets").join(path)).expect("deleted");
    project.reindex_and_invalidate(path);
    pre_extract(&mut world, &project.runtime);
    pre_extract(&mut world, &project.runtime);

    assert!(world.get::<UiTextLayout>(entity).is_none());
    assert!(world.get::<UiText>(entity).is_some(), "the text stays");
}

/// **A font file created after its text lays the text out.**
#[test]
fn a_font_file_created_after_its_text_lays_the_text_out() {
    let path = "fonts/late.ttf";
    let project = FontProject::new(&[]);
    let mut world = World::new();
    let font = khora_core::asset::AssetUUID::new_v5(path);
    let entity = FontProject::text(&mut world, font);
    pre_extract(&mut world, &project.runtime);
    assert!(world.get::<UiTextLayout>(entity).is_none(), "no file yet");

    std::fs::write(project.root.join("assets").join(path), INTER).expect("created");
    project.reindex_and_invalidate(path);
    pre_extract(&mut world, &project.runtime);
    pre_extract(&mut world, &project.runtime);

    assert_eq!(
        world.get::<UiTextLayout>(entity).expect("laid out").font,
        font
    );
}

/// **A file mid-write keeps its texts' layouts**: a font file truncated by
/// a write in progress reads as no font, and the text keeps nothing it can
/// not draw — once the write ends, the next change lays it out again.
#[test]
fn a_font_file_rewritten_lays_its_text_out_again_once_whole() {
    let path = "fonts/body.ttf";
    let project = FontProject::new(&[(path, INTER)]);
    let mut world = World::new();
    let entity = FontProject::text(&mut world, khora_core::asset::AssetUUID::new_v5(path));
    pre_extract(&mut world, &project.runtime);
    assert!(world.get::<UiTextLayout>(entity).is_some());

    let file = project.root.join("assets").join(path);
    std::fs::write(&file, &INTER[..64]).expect("half written");
    project.reindex_and_invalidate(path);
    pre_extract(&mut world, &project.runtime);
    std::fs::write(&file, INTER).expect("written");
    project.reindex_and_invalidate(path);
    pre_extract(&mut world, &project.runtime);

    assert!(
        world.get::<UiTextLayout>(entity).is_some(),
        "laid out again"
    );
}
