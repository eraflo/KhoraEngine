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

//! The decode family: assets decoded off the frame.
//!
//! The asset service files a [`DecodeRequest`] on a cache miss; the family's
//! lane, at `ExecutionPhase::BACKGROUND`, takes them, decodes each through
//! the runtime's [`AssetDecoding`], and stages each [`Decoded`] under the
//! lease of its asset's generation, for the asset intake at a frame's
//! boundary.
//!
//! [`DecodeRequest`]: khora_core::asset::DecodeRequest
//! [`AssetDecoding`]: khora_core::asset::AssetDecoding
//! [`Decoded`]: khora_core::asset::Decoded

mod decode;
mod decode_family;

pub use decode::*;
pub use decode_family::*;
