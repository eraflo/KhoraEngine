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

//! Groups all systems and data structures related to the logical concept of a "Scene".
//!
//! A scene in Khora is a collection of entities, components, and their relationships that
//! form a coherent part of the application's world. This module provides the tools
//! for defining, manipulating, and persisting these scenes.

mod serialization;
mod format;

pub use serialization::*;
pub use format::*;