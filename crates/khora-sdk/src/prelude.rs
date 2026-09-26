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

//! Common imports for game development.
//!
//! Glob-import this module to bring the everyday game-dev types into scope:
//! input ([`InputEvent`], [`KeyCode`], [`MouseButton`]), timing
//! ([`Time`], [`SharedTime`]), assets ([`AssetHandle`], [`AssetUUID`]), and
//! the [`ecs`], [`materials`], and [`math`] sub-modules. The window config
//! types [`WindowConfig`] and [`WindowIcon`] come along too.
//!
//! # Examples
//!
//! ```rust
//! use khora_sdk::prelude::*;
//! use khora_sdk::prelude::math::Vec3;
//!
//! // ECS components, math, and materials are all reachable through the prelude.
//! let _transform = ecs::Transform::from_translation(Vec3::new(0.0, 1.0, 0.0));
//! let _material = materials::StandardMaterial::default();
//! let _red = math::LinearRgba::RED;
//! ```

// SDK types
pub use crate::{WindowConfig, WindowIcon, PRIMARY_VIEWPORT};

// Assets
pub use khora_core::asset::{AssetHandle, AssetUUID};

// Memory tracking (for `#[global_allocator]`)
pub use khora_core::memory::SaaTrackingAllocator;

// Input
pub use khora_core::platform::{InputEvent, KeyCode, MouseButton};

// Per-frame timing — real frame delta, fixed sim step, interpolation alpha.
// `SharedTime` is the interior-mutable handle to cache in `setup` and read
// each frame in `update`.
pub use khora_core::time::{SharedTime, Time};

// ECS types
pub mod ecs {
    //! Core ECS types for game logic.
    pub use khora_core::ecs::entity::EntityId;
    pub use khora_core::physics::{BodyType, ColliderShape};
    pub use khora_core::renderer::light::{DirectionalLight, LightType, PointLight, SpotLight};
    pub use khora_data::ecs::{
        AudioSource, Camera, Children, Collider, Component, ComponentBundle, GlobalTransform,
        Light, MaterialRef, MeshRef, Name, Parent, ProceduralMeshKind, ProjectionType, RigidBody,
        Script, Tag, Transform, Without,
    };
    // `Script` is how an entity gets gameplay logic. It was missing from
    // this list, which meant no game could attach a behaviour through the
    // SDK at all — the only road in was the editor's generic
    // "+ Add Component" card.
    pub use khora_core::script::ScriptValue;
}

// Materials
pub mod materials {
    //! Built-in material types.
    //!
    //! [`AlphaMode`] is re-exported alongside them because it is the type of
    //! `StandardMaterial::alpha_mode`: without it a game could not select
    //! masked or blended transparency through the SDK.
    pub use khora_core::asset::{
        AlphaMode, EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial,
    };
}

// Math
pub mod math {
    //! Math types and utilities.
    pub use khora_core::math::LinearRgba;
    pub use khora_core::math::*;
}
