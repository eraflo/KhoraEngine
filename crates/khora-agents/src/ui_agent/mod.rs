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

//! Defines the UiAgent — owns `LaneKind::Ui` lanes only.
//!
//! Per CLAD, an Agent owns exactly one `LaneKind` and stores **only** its
//! own GORNA/strategy state.  All shared services (graphics device, render
//! system, fonts, text renderer, texture assets, the per-frame `UiScene`,
//! the UI image atlas) are looked up from the engine
//! [`Runtime`](khora_core::Runtime) each frame.
//!
//! The persistent `AssetUUID → AtlasRect` cache and the GPU
//! [`TextureAtlas`](khora_core::renderer::api::util::TextureAtlas) used
//! to live on this agent as `image_cache` and `image_atlas` fields. They
//! now both live in [`UiImageAtlas`](khora_data::ui::UiImageAtlas) (a
//! `Resource`). The agent owns **no** GPU state and no buffered output.

mod agent;

pub use agent::*;
