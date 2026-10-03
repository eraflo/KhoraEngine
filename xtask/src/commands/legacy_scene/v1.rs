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

//! The scene format before scene records, read once to be written again.
//!
//! A v1 scene was a recipe: a list of commands — spawn an entity, add a
//! component to it, parent it — each component's value the bincode
//! (`config::standard()`) of its `Serializable<Type>` mirror, named by its
//! bare type name. A v1 prefab was the same recipe with no header, its root
//! spawned first.
//!
//! The recipe is replayed into a world through the engine's public API, so
//! the world is then saved exactly as the editor would save it now.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use bincode::config;
use khora_core::asset::{
    AssetUUID, EmissiveMaterial, Material, StandardMaterial, UnlitMaterial, WireframeMaterial,
};
use khora_core::ecs::entity::EntityId;
use khora_core::math::LinearRgba;
use khora_sdk::khora_data::ecs::{
    self as ecs, Component, MaterialRef, MeshRef, ProceduralMeshKind, World,
};
use khora_sdk::khora_data::scene::registration_named;
use khora_sdk::khora_data::ui;
use serde::de::DeserializeOwned;

/// A v1 recipe: the commands that rebuild a scene.
#[derive(bincode::Decode)]
pub(super) struct Recipe {
    pub(super) commands: Vec<Command>,
}

/// One v1 command.
#[derive(bincode::Decode)]
pub(super) enum Command {
    Spawn {
        id: EntityId,
    },
    AddComponent {
        entity_id: EntityId,
        component_type: String,
        component_data: Vec<u8>,
    },
    SetParent {
        child_id: EntityId,
        parent_id: EntityId,
    },
}

/// Reads a v1 recipe that must be the whole of `bytes`.
pub(super) fn read_recipe(bytes: &[u8]) -> Result<Recipe> {
    let (recipe, used): (Recipe, usize) = bincode::decode_from_slice(bytes, config::standard())
        .map_err(|error| anyhow!("not a v1 recipe: {error}"))?;
    if used != bytes.len() {
        bail!("not a v1 recipe: {} byte(s) after it", bytes.len() - used);
    }
    Ok(recipe)
}

/// What replaying a recipe built.
pub(super) struct Replayed {
    pub(super) world: World,
    /// The entity spawned first: a prefab's root.
    pub(super) first: Option<EntityId>,
    /// Components the file held that the engine rebuilds rather than reads.
    pub(super) skipped: usize,
}

/// Replays `recipe` into a new world: every spawned entity authored, every
/// component decoded by its name, the hierarchy rebuilt from the `SetParent`
/// commands through the one writer of both its halves.
pub(super) fn replay(recipe: &Recipe) -> Result<Replayed> {
    let mut world = World::new();
    let mut entities: HashMap<EntityId, EntityId> = HashMap::new();
    let mut first = None;
    let mut skipped = 0;
    for command in &recipe.commands {
        match command {
            Command::Spawn { id } => {
                let entity = world.spawn(());
                world.mark_authored(entity);
                if entities.insert(*id, entity).is_some() {
                    bail!("entity {id:?} is spawned twice");
                }
                first.get_or_insert(entity);
            }
            Command::AddComponent {
                entity_id,
                component_type,
                component_data,
            } => {
                let entity = *entities.get(entity_id).ok_or_else(|| {
                    anyhow!("`{component_type}` added to {entity_id:?}, never spawned")
                })?;
                let added = add_component(&mut world, entity, component_type, component_data)
                    .with_context(|| format!("component `{component_type}`"))?;
                if !added {
                    skipped += 1;
                }
            }
            Command::SetParent {
                child_id,
                parent_id,
            } => {
                let (Some(child), Some(parent)) = (entities.get(child_id), entities.get(parent_id))
                else {
                    // A parent outside the file is not a parent.
                    continue;
                };
                if !world.set_parent(*child, Some(*parent)) {
                    bail!("{child_id:?} cannot be parented to {parent_id:?}");
                }
            }
        }
    }
    Ok(Replayed {
        world,
        first,
        skipped,
    })
}

/// Decodes the v1 value of the component named `name` and adds it to
/// `entity`. `Ok(false)` for a component the engine rebuilds instead — the
/// hierarchy's `Parent`, which `SetParent` carries, and anything derived or
/// kept while running.
fn add_component(world: &mut World, entity: EntityId, name: &str, bytes: &[u8]) -> Result<bool> {
    if name == "Parent" {
        return Ok(false);
    }
    let registration =
        registration_named(name).ok_or_else(|| anyhow!("no component is named `{name}`"))?;
    if !registration.is_saved() {
        return Ok(false);
    }
    match name {
        "MeshRef" => add(world, entity, mesh_ref(bytes)?),
        "MaterialRef" => add(world, entity, material_ref(bytes)?),
        "Transform" => plain::<ecs::SerializableTransform, ecs::Transform>(world, entity, bytes),
        "Name" => plain::<ecs::SerializableName, ecs::Name>(world, entity, bytes),
        "Tag" => plain::<ecs::SerializableTag, ecs::Tag>(world, entity, bytes),
        "Camera" => plain::<ecs::SerializableCamera, ecs::Camera>(world, entity, bytes),
        "Light" => plain::<ecs::SerializableLight, ecs::Light>(world, entity, bytes),
        "AudioSource" => {
            plain::<ecs::SerializableAudioSource, ecs::AudioSource>(world, entity, bytes)
        }
        "AudioListener" => {
            plain::<ecs::SerializableAudioListener, ecs::AudioListener>(world, entity, bytes)
        }
        "RigidBody" => plain::<ecs::SerializableRigidBody, ecs::RigidBody>(world, entity, bytes),
        "Collider" => plain::<ecs::SerializableCollider, ecs::Collider>(world, entity, bytes),
        "PhysicsMaterial" => {
            plain::<ecs::SerializablePhysicsMaterial, ecs::PhysicsMaterial>(world, entity, bytes)
        }
        "KinematicCharacterController" => plain::<
            ecs::SerializableKinematicCharacterController,
            ecs::KinematicCharacterController,
        >(world, entity, bytes),
        "ActiveEvents" => {
            plain::<ecs::SerializableActiveEvents, ecs::ActiveEvents>(world, entity, bytes)
        }
        "UiTransform" => {
            plain::<ui::SerializableUiTransform, ui::UiTransform>(world, entity, bytes)
        }
        "UiNode" => plain::<ui::SerializableUiNode, ui::UiNode>(world, entity, bytes),
        "UiStyle" => plain::<ui::SerializableUiStyle, ui::UiStyle>(world, entity, bytes),
        "UiColor" => plain::<ui::SerializableUiColor, ui::UiColor>(world, entity, bytes),
        "UiBorder" => plain::<ui::SerializableUiBorder, ui::UiBorder>(world, entity, bytes),
        "UiInteraction" => {
            plain::<ui::SerializableUiInteraction, ui::UiInteraction>(world, entity, bytes)
        }
        other => bail!(
            "a v1 `{other}` cannot be upgraded: its value holds an asset or a script machine in a \
             form only the old engine read"
        ),
    }
}

fn add<C: Component>(world: &mut World, entity: EntityId, component: C) -> Result<bool> {
    world
        .add_component(entity, component)
        .map_err(|error| anyhow!("could not be added: {error:?}"))?;
    Ok(true)
}

/// A component whose v1 bytes are its mirror's: bincode's derive and its
/// serde mode write the same bytes for the same config, for every type a
/// plain mirror holds.
fn plain<M: DeserializeOwned, C: Component + From<M>>(
    world: &mut World,
    entity: EntityId,
    bytes: &[u8],
) -> Result<bool> {
    let (mirror, used): (M, usize) =
        bincode::serde::decode_from_slice(bytes, config::standard())
            .map_err(|error| anyhow!("the value does not decode: {error}"))?;
    if used != bytes.len() {
        bail!("{} byte(s) after the value", bytes.len() - used);
    }
    add(world, entity, C::from(mirror))
}

/// A v1 mesh reference.
#[derive(bincode::Decode)]
enum V1MeshRef {
    Procedural { kind: V1MeshKind, params: [f32; 4] },
    Asset(AssetUUID),
}

/// A v1 procedural mesh kind, in v1 order.
#[derive(bincode::Decode)]
enum V1MeshKind {
    Cube,
    Sphere,
    Plane,
}

fn mesh_ref(bytes: &[u8]) -> Result<MeshRef> {
    Ok(match whole::<V1MeshRef>(bytes)? {
        V1MeshRef::Procedural { kind, params } => {
            let kind = match kind {
                V1MeshKind::Cube => ProceduralMeshKind::Cube,
                V1MeshKind::Sphere => ProceduralMeshKind::Sphere,
                V1MeshKind::Plane => ProceduralMeshKind::Plane,
            };
            MeshRef::procedural(kind, params)
        }
        V1MeshRef::Asset(uuid) => MeshRef::Asset(uuid),
    })
}

/// A v1 material reference: an inline material's type-tagged bytes, or an
/// asset's UUID.
#[derive(bincode::Decode)]
enum V1MaterialRef {
    Inline(Vec<u8>),
    Asset(AssetUUID),
}

/// A v1 inline material: its type name and the bincode of the material.
#[derive(bincode::Decode)]
struct V1Material {
    type_name: String,
    data: Vec<u8>,
}

fn material_ref(bytes: &[u8]) -> Result<MaterialRef> {
    Ok(match whole::<V1MaterialRef>(bytes)? {
        V1MaterialRef::Inline(bytes) => {
            MaterialRef::inline(material(&whole::<V1Material>(&bytes)?)?)
        }
        V1MaterialRef::Asset(uuid) => MaterialRef::Asset(uuid),
    })
}

fn material(v1: &V1Material) -> Result<Box<dyn Material>> {
    let data = &v1.data;
    Ok(match v1.type_name.as_str() {
        "StandardMaterial" => Box::new(whole::<StandardMaterial>(data)?),
        "UnlitMaterial" => Box::new(whole::<UnlitMaterial>(data)?),
        "EmissiveMaterial" => Box::new(whole::<EmissiveMaterial>(data)?),
        "WireframeMaterial" => Box::new(whole::<WireframeMaterial>(data)?),
        // What v1 wrote for a material no registration claimed: its base
        // color, read back as a standard material of that color.
        "__unknown__" => Box::new(StandardMaterial {
            base_color: whole::<LinearRgba>(data)?,
            ..StandardMaterial::default()
        }),
        other => bail!("no material type `{other}`"),
    })
}

/// Decodes a `T` that must be the whole of `bytes`.
fn whole<T: bincode::Decode<()>>(bytes: &[u8]) -> Result<T> {
    let (value, used): (T, usize) = bincode::decode_from_slice(bytes, config::standard())
        .map_err(|error| anyhow!("the value does not decode: {error}"))?;
    if used != bytes.len() {
        bail!("{} byte(s) after the value", bytes.len() - used);
    }
    Ok(value)
}
