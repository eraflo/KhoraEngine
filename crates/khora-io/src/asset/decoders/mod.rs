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

//! Concrete [`AssetDecoder`] implementations: bytes → typed asset.
//!
//! Decoders are pure CPU work; they have no GPU/IO state. They are
//! registered with the [`AssetService`] via `register_decoder` and dispatched
//! by asset type name during `load`.
//!
//! [`AssetDecoder`]: super::AssetDecoder
//! [`AssetService`]: super::AssetService

pub mod audio;
pub mod font;
pub mod mesh;
pub mod texture;

pub use audio::*;
pub use font::*;
pub use mesh::*;
pub use texture::*;
