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

//! What laying texts out costs in memory when many share one font.
//!
//! Its own test binary: it counts every allocation the process makes, and in
//! a shared binary the other tests' allocations would be counted with it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::lane::OutputDeck;
use khora_core::math::Vec2;
use khora_core::ui::text::{
    FontError, FontFamily, ShapeError, ShapeRequest, ShapedText, TextShaper,
};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, HandleComponent, SystemContext, Tick, World};
use khora_data::ui::{UiText, UiTextLayout, UiTransform};

/// The system allocator, keeping the most memory ever held at once.
struct Peak;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to `System`, which upholds the
// `GlobalAlloc` contract; the counters only observe the sizes.
unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded with the caller's layout, as `System` requires.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above with this same layout.
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

/// A shaper that reads every font and lays every text out in no glyph.
struct Accepting;

impl TextShaper for Accepting {
    fn register_font(&self, _: AssetUUID, _: Arc<[u8]>) -> Result<FontFamily, FontError> {
        Ok(FontFamily {
            name: "Any".to_owned(),
        })
    }

    fn shape(&self, _: &ShapeRequest<'_>) -> Result<ShapedText, ShapeError> {
        Ok(ShapedText {
            glyphs: Vec::new(),
            size: Vec2::ZERO,
            lines: 1,
        })
    }
}

const BODY: AssetUUID = AssetUUID::from_bytes(*b"khora:test:body!");

/// **Texts sharing a font do not copy it once each.** The resolver hands
/// every text naming a font the same handle, so a scene of many texts in one
/// font holds its file once; laying them out — registering the font once —
/// must not hold one copy of the file per text along the way.
#[test]
fn many_texts_sharing_a_font_do_not_copy_it_once_each() {
    const TEXTS: usize = 256;
    const FONT_BYTES: usize = 1 << 20;

    let mut world = World::new();
    let handle = AssetHandle::new(Font {
        name: "Body".to_owned(),
        data: vec![7; FONT_BYTES],
    });
    let entities: Vec<_> = (0..TEXTS)
        .map(|i| {
            world.spawn((
                UiTransform::default(),
                UiText {
                    content: format!("text {i}"),
                    font: BODY,
                    size: 16.0,
                    ..Default::default()
                },
                HandleComponent {
                    handle: handle.clone(),
                    uuid: BODY,
                },
            ))
        })
        .collect();
    let mut runtime = Runtime::new();
    let shaper: Arc<dyn TextShaper> = Arc::new(Accepting);
    runtime.backends.insert(shaper);
    let system = inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|registration| registration.name == "ui_text_layout")
        .expect("`ui_text_layout` is registered");
    let mut deck = OutputDeck::new();

    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    (system.run)(
        &mut world,
        &runtime,
        &mut deck,
        SystemContext { since: Tick(0) },
    );
    let grew = PEAK.load(Ordering::Relaxed).saturating_sub(before);

    assert!(
        entities
            .iter()
            .all(|entity| world.get::<UiTextLayout>(*entity).is_some()),
        "every text is laid out"
    );
    assert!(
        grew < 8 * FONT_BYTES,
        "laying out {TEXTS} texts in one {FONT_BYTES}-byte font held {grew} bytes more at its \
         peak: about {} copies of the font",
        grew / FONT_BYTES
    );
}
