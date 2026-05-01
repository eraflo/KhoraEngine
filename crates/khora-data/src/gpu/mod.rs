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

//! GPU resource management — shared cache and CPU→GPU projection.
//!
//! This module provides:
//! - [`GpuCache`]: the engine-wide, shared GPU mesh cache.
//! - [`ProjectionRegistry`]: drives CPU→GPU mesh upload before agents run.
//!
//! Both are registered into the [`ServiceRegistry`] during bootstrap and
//! must not be held as local fields inside agents.

pub mod cache;
pub mod projection;

pub use cache::GpuCache;
pub use projection::ProjectionRegistry;
