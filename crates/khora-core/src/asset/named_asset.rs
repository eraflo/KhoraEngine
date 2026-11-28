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

//! [`NamedAsset`], an asset named by its uuid and its kind.

use super::{AssetKind, AssetUUID};

/// An asset named by its uuid, with the kind it is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NamedAsset {
    /// The asset's identity.
    pub uuid: AssetUUID,
    /// The kind the asset is of.
    pub kind: AssetKind,
}
