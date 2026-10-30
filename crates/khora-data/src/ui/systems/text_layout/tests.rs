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

//! The `ui_text_layout` DataSystem, run against a shaper that records what it
//! is asked: what is shaped, when, and with what it is registered.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::math::Vec2;
use khora_core::ui::text::{
    FontError, FontFamily, ShapeError, ShapeRequest, ShapedGlyph, ShapedText, TextShaper,
};
use khora_core::Runtime;

use super::ui_text_layout;
use crate::ecs::{
    ComponentKey, DataSystemRegistration, HandleComponent, SystemContext, Tick, TickPhase, World,
};
use crate::ui::{UiText, UiTextLayout, UiTransform};

/// A font asset the tests load.
const BODY: AssetUUID = AssetUUID::from_bytes(*b"khora:test:body!");
/// A second font asset.
const TITLE: AssetUUID = AssetUUID::from_bytes(*b"khora:test:title");
/// A font asset whose file the shaper cannot read.
const BROKEN: AssetUUID = AssetUUID::from_bytes(*b"khora:test:broke");

/// Data the fake shaper refuses to register, as a real one refuses bytes that
/// are not a font file.
const UNREADABLE: &[u8] = b"not a font";

/// One `shape` call, as the shaper received it.
#[derive(Debug, Clone, PartialEq)]
struct Request {
    text: String,
    font: AssetUUID,
    size_px: f32,
    max_width: Option<f32>,
}

/// A `TextShaper` that records every call. It knows `Font::BUILT_IN` from the
/// start, as the real backends do, and every font registered with readable
/// data after; it refuses to shape in any other font.
struct FakeShaper {
    registered: Mutex<Vec<(AssetUUID, Vec<u8>)>>,
    shaped: Mutex<Vec<Request>>,
    known: Mutex<HashSet<AssetUUID>>,
}

impl FakeShaper {
    fn new() -> Self {
        Self {
            registered: Mutex::new(Vec::new()),
            shaped: Mutex::new(Vec::new()),
            known: Mutex::new(HashSet::from([Font::BUILT_IN])),
        }
    }

    /// Every registration, in call order.
    fn registrations(&self) -> Vec<(AssetUUID, Vec<u8>)> {
        self.registered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Every shaping request, in call order.
    fn requests(&self) -> Vec<Request> {
        self.shaped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Every call of either kind.
    fn calls(&self) -> usize {
        self.registrations().len() + self.requests().len()
    }
}

impl TextShaper for FakeShaper {
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<FontFamily, FontError> {
        self.registered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, data.to_vec()));
        let mut known = self.known.lock().unwrap_or_else(|e| e.into_inner());
        if &*data == UNREADABLE {
            known.remove(&id);
            return Err(FontError::Unreadable);
        }
        known.insert(id);
        Ok(FontFamily {
            name: "Fake".to_owned(),
        })
    }

    fn shape(&self, request: &ShapeRequest<'_>) -> Result<ShapedText, ShapeError> {
        self.shaped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Request {
                text: request.text.to_owned(),
                font: request.font,
                size_px: request.size_px,
                max_width: request.max_width,
            });
        if !self
            .known
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&request.font)
        {
            return Err(ShapeError::UnknownFont(request.font));
        }
        Ok(fake_shape(request.text, request.size_px))
    }
}

/// What the fake shaper returns: one glyph per character but `'\n'`, its id
/// the character's code, half an em wide, on a baseline one em below its
/// line's top.
fn fake_shape(text: &str, size_px: f32) -> ShapedText {
    let mut glyphs = Vec::new();
    let mut widest = 0usize;
    let lines: Vec<&str> = text.split('\n').collect();
    for (line, chars) in lines.iter().enumerate() {
        widest = widest.max(chars.chars().count());
        for (column, c) in chars.chars().enumerate() {
            glyphs.push(ShapedGlyph {
                id: c as u16,
                x: column as f32 * size_px * 0.5,
                y: (line + 1) as f32 * size_px,
            });
        }
    }
    ShapedText {
        glyphs,
        size: Vec2::new(widest as f32 * size_px * 0.5, lines.len() as f32 * size_px),
        lines: lines.len(),
    }
}

/// A world, a runtime and the system's last-seen tick: each `run` is one run
/// of the system as the runner makes it, handed the tick of its previous run.
struct Scene {
    world: World,
    runtime: Runtime,
    shaper: Arc<FakeShaper>,
    last_seen: Tick,
}

impl Scene {
    /// A scene whose runtime holds the fake shaper.
    fn new() -> Self {
        let shaper = Arc::new(FakeShaper::new());
        let mut runtime = Runtime::new();
        let backend: Arc<dyn TextShaper> = shaper.clone();
        runtime.backends.insert(backend);
        Self {
            world: World::new(),
            runtime,
            shaper,
            last_seen: Tick(0),
        }
    }

    /// A scene whose runtime holds no shaper.
    fn without_a_shaper() -> Self {
        Self {
            world: World::new(),
            runtime: Runtime::new(),
            shaper: Arc::new(FakeShaper::new()),
            last_seen: Tick(0),
        }
    }

    /// Spawns a placed text.
    fn text(&mut self, content: &str, font: AssetUUID, size: f32) -> EntityId {
        self.world
            .spawn((UiTransform::default(), text(content, font, size)))
    }

    /// Spawns a placed text holding the handle of its font.
    fn text_with_handle(
        &mut self,
        content: &str,
        font: AssetUUID,
        size: f32,
        data: &[u8],
    ) -> EntityId {
        self.world.spawn((
            UiTransform::default(),
            text(content, font, size),
            font_handle(font, data),
        ))
    }

    /// One run of the system.
    fn run(&mut self) {
        let tick = self.world.begin_run();
        ui_text_layout(
            &mut self.world,
            &self.runtime,
            &mut OutputDeck::new(),
            SystemContext {
                since: self.last_seen,
            },
        );
        self.world.end_run();
        self.last_seen = tick;
    }

    fn layout(&self, entity: EntityId) -> Option<UiTextLayout> {
        self.world.get::<UiTextLayout>(entity).cloned()
    }

    /// The shaping requests made after the first `from`.
    fn requests_since(&self, from: usize) -> Vec<Request> {
        self.shaper.requests()[from..].to_vec()
    }

    /// The registrations made after the first `from`.
    fn registrations_since(&self, from: usize) -> Vec<(AssetUUID, Vec<u8>)> {
        self.shaper.registrations()[from..].to_vec()
    }
}

fn text(content: &str, font: AssetUUID, size: f32) -> UiText {
    UiText {
        content: content.to_owned(),
        font,
        size,
        ..Default::default()
    }
}

fn font_handle(uuid: AssetUUID, data: &[u8]) -> HandleComponent<Font> {
    HandleComponent {
        handle: AssetHandle::new(Font {
            name: "Body".to_owned(),
            data: data.to_vec(),
        }),
        uuid,
    }
}

fn request(content: &str, font: AssetUUID, size_px: f32) -> Request {
    Request {
        text: content.to_owned(),
        font,
        size_px,
        max_width: None,
    }
}

// --- shaped once ---

/// **A text is shaped once.** Its first run lays it out; later runs where
/// nothing was written ask the shaper nothing.
#[test]
fn a_text_is_shaped_once_and_not_again_when_nothing_changed() {
    let mut scene = Scene::new();
    let entity = scene.text("Hello", Font::BUILT_IN, 16.0);

    scene.run();
    assert!(scene.layout(entity).is_some(), "laid out on its first run");
    assert_eq!(
        scene.shaper.requests(),
        vec![request("Hello", Font::BUILT_IN, 16.0)],
        "shaped once, unwrapped"
    );

    scene.run();
    scene.run();
    assert_eq!(
        scene.shaper.requests().len(),
        1,
        "nothing changed, nothing reshaped: {:?}",
        scene.shaper.requests()
    );
    assert!(scene.layout(entity).is_some(), "the layout stays");
}

/// **The layout is what the shaper returned**, with the font and size it was
/// shaped in.
#[test]
fn the_layout_holds_what_the_shaper_returned() {
    let mut scene = Scene::new();
    let entity = scene.text_with_handle("ab\ncde", BODY, 20.0, b"body v1");

    scene.run();

    let expected = fake_shape("ab\ncde", 20.0);
    let layout = scene.layout(entity).expect("laid out");
    assert_eq!(layout.font, BODY);
    assert_eq!(layout.size_px, 20.0);
    assert_eq!(layout.glyphs, expected.glyphs);
    assert_eq!(layout.size, expected.size);
    assert_eq!(layout.lines, 2);
    assert_eq!(
        scene.shaper.requests(),
        vec![request("ab\ncde", BODY, 20.0)],
        "the text's content, font and size, with no width to wrap at"
    );
}

// --- edits reshape the edited text alone ---

/// **Editing a text's content reshapes it alone.** The other text is not
/// asked for again, and its layout is what it was.
#[test]
fn editing_a_text_s_content_reshapes_it_alone() {
    let mut scene = Scene::new();
    let edited = scene.text("before", Font::BUILT_IN, 16.0);
    let other = scene.text("other", Font::BUILT_IN, 16.0);
    scene.run();
    let other_layout = scene.layout(other).expect("laid out");
    let from = scene.shaper.requests().len();

    scene
        .world
        .get_mut::<UiText>(edited)
        .expect("a text")
        .content = "after!".to_owned();
    scene.run();

    assert_eq!(
        scene.requests_since(from),
        vec![request("after!", Font::BUILT_IN, 16.0)],
        "the edited text alone is reshaped"
    );
    assert_eq!(
        scene.layout(edited).expect("laid out").glyphs,
        fake_shape("after!", 16.0).glyphs,
        "its layout is the new content's"
    );
    assert_eq!(
        scene.layout(other),
        Some(other_layout),
        "the other untouched"
    );
}

/// **Editing a text's size reshapes it alone**, at the new size.
#[test]
fn editing_a_text_s_size_reshapes_it_alone() {
    let mut scene = Scene::new();
    let edited = scene.text("grow", Font::BUILT_IN, 16.0);
    let other = scene.text("stay", Font::BUILT_IN, 16.0);
    scene.run();
    let other_layout = scene.layout(other).expect("laid out");
    let from = scene.shaper.requests().len();

    scene.world.get_mut::<UiText>(edited).expect("a text").size = 32.0;
    scene.run();

    assert_eq!(
        scene.requests_since(from),
        vec![request("grow", Font::BUILT_IN, 32.0)]
    );
    let layout = scene.layout(edited).expect("laid out");
    assert_eq!(layout.size_px, 32.0);
    assert_eq!(layout.size, fake_shape("grow", 32.0).size);
    assert_eq!(
        scene.layout(other),
        Some(other_layout),
        "the other untouched"
    );
}

/// **Editing a text's font reshapes it alone**, in the new font — once,
/// though its `UiText` and its font handle both changed.
#[test]
fn editing_a_text_s_font_reshapes_it_alone() {
    let mut scene = Scene::new();
    let edited = scene.text("restyle", Font::BUILT_IN, 16.0);
    let other = scene.text("stay", Font::BUILT_IN, 16.0);
    scene.run();
    let other_layout = scene.layout(other).expect("laid out");
    let from = scene.shaper.requests().len();

    // What an edit and the resolver after it do: the font changes, and its
    // handle is attached.
    scene.world.get_mut::<UiText>(edited).expect("a text").font = BODY;
    scene
        .world
        .add_component(edited, font_handle(BODY, b"body v1"))
        .expect("the handle attaches");
    scene.run();

    assert_eq!(
        scene.requests_since(from),
        vec![request("restyle", BODY, 16.0)],
        "the edited text alone, once, in its new font"
    );
    assert_eq!(scene.layout(edited).expect("laid out").font, BODY);
    assert_eq!(
        scene.layout(other),
        Some(other_layout),
        "the other untouched"
    );
}

// --- the font ---

/// **The built-in font needs no handle.** Every shaper knows it from its
/// construction, so nothing is registered.
#[test]
fn the_built_in_font_shapes_without_a_handle() {
    let mut scene = Scene::new();
    let entity = scene.text("Built in", Font::BUILT_IN, 16.0);

    scene.run();

    let layout = scene.layout(entity).expect("laid out without a handle");
    assert_eq!(layout.font, Font::BUILT_IN);
    assert!(
        scene.shaper.registrations().is_empty(),
        "nothing to register: {:?}",
        scene.shaper.registrations()
    );
}

/// **A text whose font is not resolved has no layout.**
#[test]
fn a_text_whose_font_is_not_resolved_has_no_layout() {
    let mut scene = Scene::new();
    let entity = scene.text("Waiting", BODY, 16.0);

    scene.run();

    assert_eq!(scene.layout(entity), None);
}

/// **A text needs its own handle**, not only a shaper that knows the font: a
/// text whose handle has not arrived yet has no layout, though another text
/// had the same font registered.
#[test]
fn a_text_without_its_own_font_handle_has_no_layout_though_another_loaded_the_font() {
    let mut scene = Scene::new();
    let loaded = scene.text_with_handle("Loaded", BODY, 16.0, b"body v1");
    let waiting = scene.text("Waiting", BODY, 16.0);

    scene.run();

    assert!(scene.layout(loaded).is_some(), "the text with its handle");
    assert_eq!(scene.layout(waiting), None, "the one without has none");
}

/// **When the font handle arrives, a later run shapes the text**, registering
/// the font first.
#[test]
fn a_text_waiting_for_its_font_is_shaped_when_the_handle_arrives() {
    let mut scene = Scene::new();
    let entity = scene.text("Waiting", BODY, 16.0);
    scene.run();
    scene.run();
    assert_eq!(scene.layout(entity), None, "nothing until the handle");

    scene
        .world
        .add_component(entity, font_handle(BODY, b"body v1"))
        .expect("the handle attaches");
    scene.run();

    let layout = scene
        .layout(entity)
        .expect("laid out once the handle is here");
    assert_eq!(layout.font, BODY);
    assert_eq!(
        scene.shaper.registrations(),
        vec![(BODY, b"body v1".to_vec())],
        "the font was registered with the handle's data"
    );
}

/// **A handle naming another font is not the text's font.** Between an edit
/// of the font and the resolver's next run, the entity still holds the old
/// font's handle: the text has no layout until the new handle arrives.
#[test]
fn a_text_whose_handle_names_another_font_has_no_layout_until_its_own_arrives() {
    let mut scene = Scene::new();
    let entity = scene.text_with_handle("Switch", BODY, 16.0, b"body v1");
    scene.run();
    assert!(scene.layout(entity).is_some());

    scene.world.get_mut::<UiText>(entity).expect("a text").font = TITLE;
    scene.run();
    assert_eq!(
        scene.layout(entity),
        None,
        "the BODY handle does not lay out a TITLE text"
    );

    assert!(scene
        .world
        .set_component(entity, font_handle(TITLE, b"title v1")));
    scene.run();
    assert_eq!(scene.layout(entity).expect("laid out").font, TITLE);
}

/// **Two texts sharing a font register it once in a run**, and a later run
/// where nothing changed does not register it again.
#[test]
fn two_texts_sharing_a_font_register_it_once_in_a_run() {
    let mut scene = Scene::new();
    let first = scene.text_with_handle("First", BODY, 16.0, b"body v1");
    let second = scene.text_with_handle("Second", BODY, 16.0, b"body v1");

    scene.run();

    assert_eq!(
        scene.shaper.registrations(),
        vec![(BODY, b"body v1".to_vec())],
        "once per font, whatever the number of texts"
    );
    assert!(scene.layout(first).is_some());
    assert!(scene.layout(second).is_some());

    scene.run();
    assert_eq!(scene.shaper.registrations().len(), 1, "not again");
}

/// **A replaced font handle is a reload.** The resolver replaces the handle
/// on every text using the font (`set_component`); the next run registers the
/// new data once and reshapes those texts, and no other.
#[test]
fn a_replaced_font_handle_re_registers_the_font_once_and_reshapes_its_texts() {
    let mut scene = Scene::new();
    let first = scene.text_with_handle("First", BODY, 16.0, b"body v1");
    let second = scene.text_with_handle("Second", BODY, 16.0, b"body v1");
    let unrelated = scene.text("Unrelated", Font::BUILT_IN, 16.0);
    scene.run();
    let registered = scene.shaper.registrations().len();
    let shaped = scene.shaper.requests().len();

    for entity in [first, second] {
        assert!(scene
            .world
            .set_component(entity, font_handle(BODY, b"body v2")));
    }
    scene.run();

    assert_eq!(
        scene.registrations_since(registered),
        vec![(BODY, b"body v2".to_vec())],
        "the new data, registered once"
    );
    let mut reshaped: Vec<String> = scene
        .requests_since(shaped)
        .into_iter()
        .map(|request| {
            assert_eq!(request.font, BODY);
            request.text
        })
        .collect();
    reshaped.sort();
    assert_eq!(
        reshaped,
        vec!["First".to_owned(), "Second".to_owned()],
        "both texts in the font, and not the unrelated one"
    );
    assert!(scene.layout(unrelated).is_some());
}

// --- losing the text or the font ---

/// **Removing a text removes its layout, and nothing else.**
#[test]
fn removing_a_text_removes_its_layout_and_nothing_else() {
    let mut scene = Scene::new();
    let removed = scene.text("Gone", Font::BUILT_IN, 16.0);
    let kept = scene.text("Kept", Font::BUILT_IN, 16.0);
    scene.run();
    let kept_layout = scene.layout(kept).expect("laid out");

    scene
        .world
        .remove_component::<UiText>(removed)
        .expect("the text is removed");
    scene.run();

    assert_eq!(scene.layout(removed), None, "no text, no layout");
    assert!(
        scene.world.get::<UiTransform>(removed).is_some(),
        "the entity keeps what is not the text's"
    );
    assert_eq!(scene.layout(kept), Some(kept_layout), "the other untouched");
}

/// **A text losing its font handle loses its layout.**
#[test]
fn a_text_losing_its_font_handle_loses_its_layout() {
    let mut scene = Scene::new();
    let entity = scene.text_with_handle("Unloaded", BODY, 16.0, b"body v1");
    scene.run();
    assert!(scene.layout(entity).is_some());

    scene
        .world
        .remove_component::<HandleComponent<Font>>(entity)
        .expect("the handle is removed");
    scene.run();

    assert_eq!(scene.layout(entity), None);
    assert!(
        scene.world.get::<UiText>(entity).is_some(),
        "the text stays"
    );
}

// --- no shaper, a refusing shaper ---

/// **Without a shaper the system does nothing**, and panics on nothing.
#[test]
fn without_a_shaper_the_system_does_nothing() {
    let mut scene = Scene::without_a_shaper();
    let built_in = scene.text("Built in", Font::BUILT_IN, 16.0);
    let loaded = scene.text_with_handle("Loaded", BODY, 16.0, b"body v1");

    scene.run();
    scene.run();

    assert_eq!(scene.layout(built_in), None);
    assert_eq!(scene.layout(loaded), None);
}

/// **A refusal is not retried every frame.** A font the shaper cannot read
/// leaves its text without a layout; the runs after, where nothing changed,
/// ask the shaper nothing.
#[test]
fn a_shaper_refusal_is_not_retried_every_frame() {
    let mut scene = Scene::new();
    let entity = scene.text_with_handle("Broken", BROKEN, 16.0, UNREADABLE);

    scene.run();
    assert_eq!(scene.layout(entity), None, "refused, so no layout");
    assert_eq!(
        scene.shaper.registrations(),
        vec![(BROKEN, UNREADABLE.to_vec())],
        "the font was offered to the shaper"
    );
    let calls = scene.shaper.calls();

    for _ in 0..5 {
        scene.run();
    }
    assert_eq!(scene.shaper.calls(), calls, "nothing retried");
    assert_eq!(scene.layout(entity), None);
}

/// **A refused reshape leaves no layout**: the text's old layout, from a font
/// it no longer uses, is removed rather than kept.
#[test]
fn a_refused_reshape_removes_the_old_layout() {
    let mut scene = Scene::new();
    let entity = scene.text("Was fine", Font::BUILT_IN, 16.0);
    scene.run();
    assert!(scene.layout(entity).is_some());

    scene.world.get_mut::<UiText>(entity).expect("a text").font = BROKEN;
    scene
        .world
        .add_component(entity, font_handle(BROKEN, UNREADABLE))
        .expect("the handle attaches");
    scene.run();

    assert_eq!(scene.layout(entity), None);
}

// --- the registration ---

/// **Registered in `PreExtract`, after the asset resolver, observing the
/// text and its font handle**: the resolver attaches and replaces the font
/// handles this system reads, and the two observed components are what it
/// reads `Changed` of.
#[test]
fn the_registration_runs_in_pre_extract_after_the_asset_resolver_observing_text_and_font() {
    let registration = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|registration| registration.name == "ui_text_layout")
        .expect("a DataSystem named `ui_text_layout` is registered");

    assert_eq!(registration.phase, TickPhase::PreExtract);
    assert!(
        registration.runs_after.contains(&"asset_resolver"),
        "after the asset resolver: {:?}",
        registration.runs_after
    );
    let observed: Vec<ComponentKey> = registration.observes.iter().map(|key| key()).collect();
    assert!(
        observed.contains(&ComponentKey::of::<UiText>()),
        "observes UiText"
    );
    assert!(
        observed.contains(&ComponentKey::of::<HandleComponent<Font>>()),
        "observes the font handle"
    );
}

// --- a font registered again ---

/// **A font registered again reshapes every text laid out in it.** The shaper
/// holds one font per id, so registering new data under an id changes the
/// font of every text naming it — not only of the entity whose handle
/// brought the data. That happens when a text is spawned after its font's
/// file changed: the resolver loads the new file for the new text, while the
/// older texts keep the handle they had. Their layouts were made from data
/// the shaper no longer holds, and the glyph ids in them name glyphs of the
/// old file.
#[test]
fn a_font_registered_again_reshapes_every_text_laid_out_in_it() {
    let mut scene = Scene::new();
    scene.text_with_handle("First", BODY, 16.0, b"body v1");
    scene.text_with_handle("Second", BODY, 16.0, b"body v1");
    scene.run();
    let shaped = scene.shaper.requests().len();

    scene.text_with_handle("Third", BODY, 16.0, b"body v2");
    scene.run();

    assert_eq!(
        scene.shaper.registrations().last(),
        Some(&(BODY, b"body v2".to_vec())),
        "the shaper now holds the new data under BODY"
    );
    let mut reshaped: Vec<String> = scene
        .requests_since(shaped)
        .into_iter()
        .map(|request| request.text)
        .collect();
    reshaped.sort();
    assert_eq!(
        reshaped,
        vec!["First".to_owned(), "Second".to_owned(), "Third".to_owned()],
        "every text in BODY is laid out in what BODY now is"
    );
}

/// **A refused font laid out once its handle is replaced by a readable
/// one**: the refusal is not remembered past the handle that caused it.
#[test]
fn a_refused_font_lays_its_text_out_once_a_readable_handle_replaces_it() {
    let mut scene = Scene::new();
    let entity = scene.text_with_handle("Fixed", BROKEN, 16.0, UNREADABLE);
    scene.run();
    assert_eq!(scene.layout(entity), None, "refused first");

    assert!(scene
        .world
        .set_component(entity, font_handle(BROKEN, b"broken, fixed")));
    scene.run();

    assert_eq!(
        scene.layout(entity).expect("laid out once readable").font,
        BROKEN
    );
}

// --- order and moves ---

/// **Laying texts out keeps them in the order they were spawned.** Attaching
/// a layout moves the entity's Ui row to another page, so the order the
/// layouts are attached in becomes the order every Ui query reads those
/// entities in — and the UI flow's sort by `z_index` is stable, so texts of
/// equal `z_index` are drawn in it. That order must not depend on a hash
/// seed: the same world must lay out, and draw, the same way every time.
#[test]
fn laying_texts_out_keeps_them_in_the_order_they_were_spawned() {
    for _ in 0..4 {
        let mut scene = Scene::new();
        let spawned: Vec<EntityId> = (0..32)
            .map(|i| scene.text(&format!("text {i}"), Font::BUILT_IN, 16.0))
            .collect();

        scene.run();

        let read: Vec<EntityId> = scene
            .world
            .query::<(EntityId, &UiText, &UiTextLayout)>()
            .map(|(entity, ..)| entity)
            .collect();
        assert_eq!(read, spawned, "the Ui rows are read in spawn order");
    }
}

/// **An edit is seen though the text moved page before the run**: gaining
/// another Ui component between the edit and the run moves the text's row,
/// and the row carries its edit with it.
#[test]
fn an_edit_is_seen_though_the_text_moved_page_before_the_run() {
    let mut scene = Scene::new();
    let entity = scene.text("before", Font::BUILT_IN, 16.0);
    scene.run();
    let from = scene.shaper.requests().len();

    scene
        .world
        .get_mut::<UiText>(entity)
        .expect("a text")
        .content = "after".to_owned();
    scene
        .world
        .add_component(entity, crate::ui::UiColor::default())
        .expect("the colour attaches");
    scene.run();

    assert_eq!(
        scene.requests_since(from),
        vec![request("after", Font::BUILT_IN, 16.0)]
    );
    assert_eq!(
        scene.layout(entity).expect("laid out").glyphs,
        fake_shape("after", 16.0).glyphs
    );
}

// --- persistence ---

/// **A layout is never saved**: it is `Derived`, made again from the text
/// whenever a world holds one, so neither a scene nor a game save keeps it.
#[test]
fn a_layout_is_derived_and_never_saved() {
    let registration =
        crate::scene::registration_of("UiTextLayout").expect("UiTextLayout is registered by name");
    assert_eq!(
        registration.provenance,
        crate::ecs::ComponentProvenance::Derived
    );
    assert!(!registration.is_saved(), "never in a scene");
    assert!(!registration.is_resumed(), "never in a game save");
}

/// **A text spawned in a font already registered reshapes it alone.** The
/// resolver hands every text naming a font the same handle; a new text's
/// handle is new on its entity, but the font's data is what the shaper
/// already holds. Registering it again, and reshaping every text in the font,
/// would make each text spawned in a custom font — a chat line, a damage
/// number — cost a parse of the font file and a reshape of all its texts.
#[test]
fn a_text_spawned_in_a_font_already_registered_reshapes_it_alone() {
    let mut scene = Scene::new();
    let shared = font_handle(BODY, b"body v1");
    for content in ["First", "Second"] {
        scene.world.spawn((
            UiTransform::default(),
            text(content, BODY, 16.0),
            shared.clone(),
        ));
    }
    scene.run();
    let registered = scene.shaper.registrations().len();
    let shaped = scene.shaper.requests().len();

    let third = scene.world.spawn((
        UiTransform::default(),
        text("Third", BODY, 16.0),
        shared.clone(),
    ));
    scene.run();

    assert!(scene.layout(third).is_some(), "the new text is laid out");
    assert_eq!(
        scene.requests_since(shaped),
        vec![request("Third", BODY, 16.0)],
        "the new text alone is shaped"
    );
    assert_eq!(
        scene.registrations_since(registered),
        vec![],
        "the font's data did not change: nothing to register"
    );
}

/// **A handle named after the built-in font does not replace it.** The
/// built-in face is no asset: every text naming it shapes with the one the
/// shaper was built with, whatever handle some entity carries under its id.
/// A refused registration under that id would drop the built-in face from
/// the shaper, and every built-in text with it.
#[test]
fn a_handle_named_after_the_built_in_font_does_not_replace_it() {
    let mut scene = Scene::new();
    let entity = scene.text("Built in", Font::BUILT_IN, 16.0);
    scene.run();
    assert!(scene.layout(entity).is_some());

    scene.world.spawn(font_handle(Font::BUILT_IN, UNREADABLE));
    scene.run();

    assert!(
        scene
            .shaper
            .registrations()
            .iter()
            .all(|(id, _)| *id != Font::BUILT_IN),
        "the built-in face is never registered: {:?}",
        scene.shaper.registrations()
    );
    assert!(
        scene.layout(entity).is_some(),
        "the built-in text keeps its layout"
    );
}

// --- the fonts new this run: the rule both text systems register by ---

/// The fonts `new_fonts` names for what changed since the last call, as
/// `(font, file)`, sorted; one call stands for one run of a system.
fn fonts_new_this_run(scene: &mut Scene) -> Vec<(AssetUUID, Vec<u8>)> {
    let tick = scene.world.begin_run();
    let fonts = super::new_fonts(&scene.world, scene.last_seen);
    scene.world.end_run();
    scene.last_seen = tick;
    let mut fonts: Vec<(AssetUUID, Vec<u8>)> = fonts
        .into_iter()
        .map(|(font, handle)| (font, handle.data.clone()))
        .collect();
    fonts.sort();
    fonts
}

/// **A font loaded is new once**, whatever the number of texts holding it,
/// and not on a run where nothing changed.
#[test]
fn a_font_loaded_is_new_once_whatever_the_number_of_its_texts() {
    let mut scene = Scene::new();
    scene.text_with_handle("First", BODY, 16.0, b"body v1");
    scene.text_with_handle("Second", BODY, 16.0, b"body v1");

    assert_eq!(
        fonts_new_this_run(&mut scene),
        vec![(BODY, b"body v1".to_vec())]
    );
    assert_eq!(fonts_new_this_run(&mut scene), vec![], "nothing changed");
}

/// **Two fonts loaded in one run are both new**, each once.
#[test]
fn two_fonts_loaded_in_one_run_are_both_new() {
    let mut scene = Scene::new();
    scene.text_with_handle("Body", BODY, 16.0, b"body v1");
    scene.text_with_handle("Title", TITLE, 16.0, b"title v1");

    let mut expected = vec![(BODY, b"body v1".to_vec()), (TITLE, b"title v1".to_vec())];
    expected.sort();
    assert_eq!(fonts_new_this_run(&mut scene), expected);
}

/// **A text spawned sharing a held file brings no new font**: the resolver
/// hands it the handle the other texts hold.
#[test]
fn a_text_spawned_sharing_a_held_file_brings_no_new_font() {
    let mut scene = Scene::new();
    let shared = font_handle(BODY, b"body v1");
    scene.world.spawn((
        UiTransform::default(),
        text("First", BODY, 16.0),
        shared.clone(),
    ));
    fonts_new_this_run(&mut scene);

    scene.world.spawn((
        UiTransform::default(),
        text("Second", BODY, 16.0),
        shared.clone(),
    ));

    assert_eq!(fonts_new_this_run(&mut scene), vec![]);
}

/// **A replaced handle brings its font once**, though every text in the font
/// had its handle replaced.
#[test]
fn a_replaced_handle_brings_its_font_once_whatever_the_number_of_its_texts() {
    let mut scene = Scene::new();
    let first = scene.text_with_handle("First", BODY, 16.0, b"body v1");
    let second = scene.text_with_handle("Second", BODY, 16.0, b"body v1");
    fonts_new_this_run(&mut scene);

    for entity in [first, second] {
        assert!(scene
            .world
            .set_component(entity, font_handle(BODY, b"body v2")));
    }

    assert_eq!(
        fonts_new_this_run(&mut scene),
        vec![(BODY, b"body v2".to_vec())]
    );
}

/// **A text spawned with a new file of a held font brings that file**: the
/// resolver loaded the font again for it, the older texts keep the file they
/// had.
#[test]
fn a_text_spawned_with_a_new_file_of_a_held_font_brings_it() {
    let mut scene = Scene::new();
    scene.text_with_handle("First", BODY, 16.0, b"body v1");
    fonts_new_this_run(&mut scene);

    scene.text_with_handle("Second", BODY, 16.0, b"body v2");

    assert_eq!(
        fonts_new_this_run(&mut scene),
        vec![(BODY, b"body v2".to_vec())]
    );
}

/// **The built-in font is never new**, whatever handle carries its id.
#[test]
fn the_built_in_font_is_never_new() {
    let mut scene = Scene::new();
    scene.world.spawn(font_handle(Font::BUILT_IN, UNREADABLE));

    assert_eq!(fonts_new_this_run(&mut scene), vec![]);
}
