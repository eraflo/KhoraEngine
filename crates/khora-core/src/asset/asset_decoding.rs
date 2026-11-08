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

//! Who decodes assets for the `loading` family.

use std::any::TypeId;

use crate::lane::Lease;

use super::{AssetUUID, DecodeRequest, Decoded};

/// Decodes the assets a [`DecodeRequest`] names, off the frame, from the
/// `loading` family's lane: it touches no `World` and fills no cache.
pub trait AssetDecoding: Send + Sync {
    /// Decodes `uuid` as the Rust type `type_id`. Never panics on bad data:
    /// a failure is an `Err` in [`Decoded::value`].
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded;

    /// Decodes `request` as [`decode`](Self::decode) does, and says what the
    /// result holds to: a [`Lease`] that stops holding once a file the decode
    /// read changed after the request was filed. None, by default.
    fn decode_leased(&self, request: &DecodeRequest) -> (Decoded, Option<Lease>) {
        (self.decode(&request.uuid, request.type_id), None)
    }
}
