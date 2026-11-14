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

//! The [`AssetDecoder`]s, one per asset type: bytes → typed asset, CPU work only.
//!
//! See the book: concepts/assets.md, "The pipeline".
//!
//! [`AssetDecoder`]: super::AssetDecoder

pub mod audio;
pub mod font;
pub mod material;
pub mod mesh;
pub mod prefab;
pub mod script;
pub mod shader;
pub mod texture;

pub use audio::*;
pub use font::*;
pub use material::*;
pub use mesh::*;
pub use prefab::*;
pub use script::*;
pub use shader::*;
pub use texture::*;
