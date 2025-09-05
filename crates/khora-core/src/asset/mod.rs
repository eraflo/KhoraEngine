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

//! Provides the foundational traits and primitive types for Khora's asset system.
//!
//! This module defines the "common language" for all asset-related operations in the
//! engine. It contains the core contracts that other crates will implement or use,
//! but it has no knowledge of how assets are loaded or stored.
//!
//! The key components are:
//! - The [`Asset`] trait: A marker for all types that can be treated as assets.
//! - Stable, unique identifiers used to reference assets throughout the engine.
//! - Abstract interfaces for loading and managing assets.
//!
//! These low-level primitives are the foundation upon which higher-level systems,
//! such as an asset database or a virtual file system (VFS), are built in other
//! crates.

mod handle;
mod materials;
mod metadata;
mod uuid;

pub use handle::*;
pub use materials::*;
pub use metadata::*;
pub use uuid::*;

/// A marker trait for types that can be managed by the asset system.
///
/// This trait's primary purpose is to categorize a type, making it eligible for
/// use within the engine's asset infrastructure (e.g., in a `Handle<T>`).
///
/// The supertraits enforce critical safety guarantees:
/// - `Send` + `Sync`: The asset type can be safely shared and sent between threads.
///   This is essential for background loading.
/// - `'static`: The asset type does not contain any non-static references, ensuring
///   it can be stored for the lifetime of the application.
///
/// # Examples
///
/// ```
/// use khora_core::asset::Asset;
///
/// // A simple struct representing a texture.
/// struct Texture {
///     // ... fields
/// }
///
/// // By implementing Asset, `Texture` can now be used by the asset system.
/// impl Asset for Texture {}
/// ```
pub trait Asset: Send + Sync + 'static {}
