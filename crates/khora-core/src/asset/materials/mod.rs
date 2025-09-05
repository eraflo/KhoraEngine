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

//! Defines the core traits and basic materials for the rendering system.

mod unlit;

pub use unlit::*;

use super::Asset;

/// A trait for types that can be used as a material.
///
/// A material defines the surface properties of an object being rendered,
/// influencing how it interacts with light and determining which shader
// (`RenderPipeline`) is used to draw it.
pub trait Material: Asset {}

/// This is the key to our type-erased material handle system.
/// We explicitly tell the compiler that a boxed, dynamic Material trait
/// object can itself be treated as a valid Asset. This allows it to be
/// stored inside an AssetHandle.
impl Asset for Box<dyn Material> {}
