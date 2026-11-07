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

//! What one decode of an asset produced.

use std::any::{Any, TypeId};

use crate::event::Supersedes;
use crate::lane::Staged;

use super::AssetUUID;

/// What one decode of an asset produced: staged by the decode family, taken
/// at a frame's boundary by the asset intake.
#[derive(Debug)]
pub struct Decoded {
    /// The asset decoded.
    pub uuid: AssetUUID,
    /// The Rust type asked for (`TypeId::of::<A>()`): `value`, when `Ok`,
    /// holds an `A`.
    pub type_id: TypeId,
    /// The decoded asset, or why it could not be decoded (not indexed, no
    /// decoder, unreadable bytes, integrity failure, another type than asked
    /// for).
    pub value: Result<Box<dyn Any + Send>, String>,
    /// Each container file the decode opened (a glTF read for one of its
    /// parts, or decoded whole), with the files it read besides its own, as
    /// asset keys. Empty for a file that is no container.
    pub external_files: Vec<(AssetUUID, Vec<String>)>,
}

impl Supersedes for Decoded {}

/// Staged by the decode family, taken by the asset intake.
impl Staged for Decoded {}
