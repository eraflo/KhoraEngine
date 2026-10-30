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

//! Every text is shaped, and reshaped only when it changes.
//!
//! The `ui_text_layout` DataSystem turns each entity's `UiText` into a
//! [`UiTextLayout`](crate::ui::UiTextLayout) through the runtime's
//! `TextShaper`, for the entities whose `UiText` or `HandleComponent<Font>`
//! changed since its last run.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::ui::text::{ShapeRequest, TextShaper};
use khora_core::Runtime;

use crate::ecs::{
    Changed, ComponentKey, DataSystemRegistration, HandleComponent, SystemContext, Tick, TickPhase,
    World,
};
use crate::ui::{UiText, UiTextLayout};

/// Shapes the texts whose `UiText` or font handle changed since `ctx.since`,
/// and registers with the shaper the fonts whose handle changed.
pub(crate) fn ui_text_layout(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    ctx: SystemContext,
) {
    let Some(shaper) = runtime.backends.get::<Arc<dyn TextShaper>>() else {
        return;
    };

    // 1. The fonts whose file is new — loaded, or replaced by a hot reload —
    //    registered once each, their file copied once, whatever number of
    //    texts use them.
    let changed_handles: Vec<EntityId> = world
        .query::<(EntityId, Changed<HandleComponent<Font>>)>()
        .since(ctx.since)
        .map(|(entity, _)| entity)
        .collect();
    let mut readable: HashMap<AssetUUID, bool> = HashMap::new();
    for (font, handle) in new_fonts(world, ctx.since) {
        let registered = match shaper.register_font(font, Arc::from(&handle.data[..])) {
            Ok(_) => true,
            Err(error) => {
                log::warn!("ui_text_layout: font {font} cannot be read: {error}");
                false
            }
        };
        readable.insert(font, registered);
    }

    // 2. The texts to shape: their text changed, their font handle did, or
    //    their font was registered again — the shaper holds one file per
    //    font, so every text in that font is now shaped with the new one.
    let mut dirty: HashSet<EntityId> = world
        .query::<(EntityId, Changed<UiText>)>()
        .since(ctx.since)
        .map(|(entity, _)| entity)
        .collect();
    dirty.extend(changed_handles.iter().copied());
    if !readable.is_empty() {
        dirty.extend(
            world
                .query::<(EntityId, &UiText)>()
                .filter(|(_, text)| readable.contains_key(&text.font))
                .map(|(entity, _)| entity),
        );
    }
    // Written in one order whatever the hash seed: attaching a layout moves
    // the entity's row, and the rows' order is the order things draw in.
    let mut dirty: Vec<EntityId> = dirty.into_iter().collect();
    dirty.sort_by_key(|entity| (entity.index, entity.generation));

    // 3. A layout outlives nothing it was made from: its text gone, or the
    //    font it needs no longer on the entity. Removing a component moves a
    //    row — it is not a write — so this is checked for every layout.
    let stale: Vec<EntityId> = world
        .query::<(EntityId, &UiTextLayout)>()
        .map(|(entity, _)| entity)
        .filter(|entity| {
            dirty
                .binary_search_by_key(&(entity.index, entity.generation), |e| {
                    (e.index, e.generation)
                })
                .is_err()
                && font_of(world, *entity).is_none()
        })
        .collect();

    // 4. Shape what is dirty; a text that cannot be shaped has no layout.
    let mut shaped = Vec::new();
    let mut unshaped = stale;
    for entity in dirty {
        let Some(text) = world.get::<UiText>(entity) else {
            unshaped.push(entity);
            continue;
        };
        let Some(font) = font_of(world, entity) else {
            unshaped.push(entity);
            continue;
        };
        if readable.get(&font) == Some(&false) {
            unshaped.push(entity);
            continue;
        }
        let request = ShapeRequest {
            text: &text.content,
            font,
            size_px: text.size,
            max_width: None,
        };
        match shaper.shape(&request) {
            Ok(text_layout) => shaped.push((
                entity,
                UiTextLayout {
                    font,
                    size_px: request.size_px,
                    size: text_layout.size,
                    lines: text_layout.lines,
                    glyphs: text_layout.glyphs,
                },
            )),
            Err(error) => {
                // Logged when the text or its font changes, not every frame:
                // an unchanged text is not shaped again.
                log::warn!("ui_text_layout: {entity:?} cannot be shaped: {error}");
                unshaped.push(entity);
            }
        }
    }

    for (entity, layout) in shaped {
        if world.get::<UiTextLayout>(entity).is_some() {
            world.set_component(entity, layout);
        } else if let Err(error) = world.add_component(entity, layout) {
            log::warn!("ui_text_layout: attaching the layout of {entity:?} failed: {error:?}");
        }
    }
    for entity in unshaped {
        if world.get::<UiTextLayout>(entity).is_some() {
            let _ = world.remove_component::<UiTextLayout>(entity);
        }
    }
}

/// The font `entity`'s text is shaped with: the built-in one, or the font its
/// own handle holds when that handle names the text's font. `None` while the
/// font is not resolved on the entity, or without a text.
fn font_of(world: &World, entity: EntityId) -> Option<AssetUUID> {
    let text = world.get::<UiText>(entity)?;
    if text.font == Font::BUILT_IN {
        return Some(Font::BUILT_IN);
    }
    let handle = world.get::<HandleComponent<Font>>(entity)?;
    (handle.uuid == text.font).then_some(text.font)
}

/// The fonts whose file is new since `since`: each font a changed
/// `HandleComponent<Font>` holds a file of that no handle left unchanged
/// since then holds. A text spawned in a font shares the handle the others
/// hold (the same `Arc`), so it brings no new file; a reload replaces the
/// handle on every entity with a new one. `Font::BUILT_IN` is never new — it
/// is the engine's, never a handle's to replace.
///
/// Each font once, with the first changed handle in page order speaking for
/// it. The rule both text systems register fonts by: the shaper's
/// (`ui_text_layout`) and the rasteriser's (`glyphs_on_gpu`).
pub(crate) fn new_fonts(world: &World, since: Tick) -> Vec<(AssetUUID, AssetHandle<Font>)> {
    let changed: Vec<(EntityId, AssetUUID, AssetHandle<Font>)> = world
        .query::<(
            EntityId,
            &HandleComponent<Font>,
            Changed<HandleComponent<Font>>,
        )>()
        .since(since)
        .map(|(entity, handle, _)| (entity, handle.uuid, handle.handle.clone()))
        .collect();
    if changed.is_empty() {
        return Vec::new();
    }
    let changed_entities: HashSet<EntityId> = changed.iter().map(|(entity, ..)| *entity).collect();
    let held_before: Vec<(AssetUUID, AssetHandle<Font>)> = world
        .query::<(EntityId, &HandleComponent<Font>)>()
        .filter(|(entity, _)| !changed_entities.contains(entity))
        .map(|(_, handle)| (handle.uuid, handle.handle.clone()))
        .collect();
    let mut seen: HashSet<AssetUUID> = HashSet::new();
    changed
        .into_iter()
        .filter(|(_, font, handle)| {
            *font != Font::BUILT_IN
                && !held_before
                    .iter()
                    .any(|(held, data)| held == font && data == handle)
        })
        .filter(|(_, font, _)| seen.insert(*font))
        .map(|(_, font, handle)| (font, handle))
        .collect()
}

inventory::submit! {
    DataSystemRegistration {
        name: "ui_text_layout",
        phase: TickPhase::PreExtract,
        run: ui_text_layout,
        order_hint: 0,
        runs_after: &["asset_resolver"],
        observes: &[
            ComponentKey::of::<UiText>,
            ComponentKey::of::<HandleComponent<Font>>,
        ],
    }
}

#[cfg(test)]
mod tests;
