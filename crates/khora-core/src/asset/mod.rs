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

//! Asset management module.
//! Contains things use for the VFS (Virtual File System)
//! This includes asset handles, metadata, and UUID generation.

mod handle;
mod metadata;
mod uuid;

pub use handle::*;
pub use metadata::*;
pub use uuid::*;

/// A marker trait for types that can be managed by the asset system.
///
/// To be considered an asset, a type must be thread-safe (`Send + Sync`)
/// and have a static lifetime (`'static`). This ensures that assets can be
/// safely loaded in background threads and shared across various engine subsystems.
pub trait Asset: Send + Sync + 'static {}