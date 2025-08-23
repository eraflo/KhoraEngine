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

//! Provides abstractions over platform-specific functionalities.
//!
//! This module contains traits and types that define a common, engine-wide interface
//! for interacting with the underlying operating system and its features, such as
//! windowing, input, and filesystem access.
//!
//! The goal is to keep the core engine logic in crates like `khora-agents` and
//! `khora-lanes` completely decoupled from any specific platform implementation
//! (like Winit or SDL2), which are handled in `khora-infra`.

pub mod window;
