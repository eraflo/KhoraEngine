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

//! The UI's asset references resolved: a text's font into its handle, an
//! image's texture into the `AssetStore`, where the UI agent uploads it.
//!
//! The built-in face ([`Font::BUILT_IN`]) is no asset and gets no handle. A
//! font or image that failed is asked for again only once an asset changes.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID};
use khora_core::ecs::entity::EntityId;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::Runtime;
use khora_data::ecs::{HandleComponent, World};
use khora_data::ui::{UiImage, UiText};
use khora_data::AssetStore;

use crate::asset::{AssetService, Requested};

/// What the resolver does to one text's font handle.
enum FontChange {
    /// Attach (or replace) the handle of the font the text names.
    Resolve {
        entity: EntityId,
        uuid: AssetUUID,
        handle: AssetHandle<Font>,
        had_stale_handle: bool,
    },
    /// The text names the built-in face: drop the handle it still carries.
    Drop(EntityId),
}

pub(crate) fn resolve_ui_fonts(world: &mut World, runtime: &Runtime) {
    let asset_service = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned();

    // Compare each text's font with its resolved handle while the
    // query borrows the world read-only.
    let mut changes: Vec<FontChange> = Vec::new();
    {
        let query = world.query::<(EntityId, &UiText, Option<&HandleComponent<Font>>)>();
        for (entity, text, current) in query {
            if text.font == Font::BUILT_IN {
                if current.is_some() {
                    changes.push(FontChange::Drop(entity));
                }
                continue;
            }
            if current.map(|h| h.uuid) == Some(text.font) {
                continue;
            }
            let Some(service) = &asset_service else {
                log::error!(
                    "asset_resolver: AssetService missing; cannot resolve font {:?} on \
                     entity {entity:?}.",
                    text.font
                );
                continue;
            };
            let requested = match service.lock() {
                Ok(mut svc) => svc.request::<Font>(&text.font),
                Err(_) => {
                    log::error!("asset_resolver: AssetService mutex poisoned.");
                    continue;
                }
            };
            // Decoded off the frame: the text waits for it. One that failed
            // (logged when it landed) is asked for again once an asset
            // changes.
            if let Requested::Ready(handle) = requested {
                changes.push(FontChange::Resolve {
                    entity,
                    uuid: text.font,
                    handle,
                    had_stale_handle: current.is_some(),
                });
            }
        }
    }

    if changes.is_empty() {
        return;
    }

    // Then publish the fonts and tag the entities.
    let fonts = runtime
        .resources
        .get::<AssetStore>()
        .map(|s| s.store::<Font>());
    for change in changes {
        match change {
            FontChange::Resolve {
                entity,
                uuid,
                handle,
                had_stale_handle,
            } => {
                if let Some(fonts) = &fonts {
                    fonts
                        .write()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(uuid, handle.clone());
                }
                let component = HandleComponent { handle, uuid };
                if had_stale_handle {
                    world.set_component(entity, component);
                } else if let Err(e) = world.add_component(entity, component) {
                    log::warn!("asset_resolver: attaching font handle on {entity:?} failed: {e:?}");
                }
            }
            FontChange::Drop(entity) => {
                if let Err(e) = world.remove_component::<HandleComponent<Font>>(entity) {
                    log::trace!(
                        "asset_resolver: dropping stale font handle on {entity:?} skipped: {e:?}"
                    );
                }
            }
        }
    }
}

pub(crate) fn resolve_ui_images(world: &World, runtime: &Runtime) {
    let Some(store) = runtime.resources.get::<AssetStore>() else {
        return;
    };
    let textures = store.store::<CpuTexture>();

    let missing: BTreeSet<AssetUUID> = {
        let held = textures.read().unwrap_or_else(|e| e.into_inner());
        world
            .query::<&UiImage>()
            .map(|image| image.texture)
            .filter(|uuid| !held.contains(uuid))
            .collect()
    };
    if missing.is_empty() {
        return;
    }

    let Some(service) = runtime.services.get::<Arc<Mutex<AssetService>>>() else {
        log::error!("asset_resolver: AssetService missing; cannot load UI images.");
        return;
    };
    for uuid in missing {
        let requested = match service.lock() {
            Ok(mut svc) => svc.request::<CpuTexture>(&uuid),
            Err(_) => {
                log::error!("asset_resolver: AssetService mutex poisoned.");
                return;
            }
        };
        // One being decoded off the frame is stored when it lands; one that
        // failed is asked for again once an asset changes.
        if let Requested::Ready(texture) = requested {
            textures
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .insert(uuid, texture);
        }
    }
}

#[cfg(test)]
mod tests;
