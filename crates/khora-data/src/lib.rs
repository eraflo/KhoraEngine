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

//! # Khora Data
//!
//! Data management systems including layouts, assets, and streaming.

#![warn(missing_docs)]

pub mod assets;
pub mod ecs;
pub mod flow;
pub mod gpu;
pub mod physics;
pub mod render;
pub mod scene;
pub mod ui;

pub use gpu::{GpuCache, ProjectionRegistry};
pub use ui::components::*;
// pub use ui::layout_view::*; // Temporarily commented out if unused or fix path
