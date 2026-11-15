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

//! # Khora Lanes
//!
//! The engine's lanes: the hot-path work of each family, grouped by family,
//! with each family's outputs and its registration.
//!
//! See the book: concepts/agents-and-lanes.md, "Lanes are layers".

#![warn(missing_docs)]

pub mod audio;
pub mod loading;
pub mod overlay;
pub mod physics;
pub mod render;
pub mod script;
pub mod shadows;
pub mod skybox;
pub mod ui;

#[cfg(test)]
pub(crate) mod tests;
