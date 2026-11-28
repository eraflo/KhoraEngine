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

//! [`AssetRef`], the one reference to an asset of a kind.

mod registered;
mod saved;

use khora_core::asset::{Asset, AssetUUID, LocalSource};

/// A reference to an asset of kind `A`: a file of the project, a value the
/// reference carries itself, or a value made in code.
pub enum AssetRef<A: Asset> {
    /// An asset of the project, by its stable uuid.
    Asset(AssetUUID),
    /// A value the reference carries itself, named by its content.
    Local(A::LocalSource),
    /// A value made in code, whose only copy is the asset store's entry under this uuid.
    ///
    /// A scene or a game save leaves it out and reports it not saved; a
    /// snapshot, read back in the same session, keeps it.
    Made(AssetUUID),
}

impl<A: Asset> AssetRef<A> {
    /// Returns the uuid the reference names: the asset's, the local source's, or the made value's.
    pub fn uuid(&self) -> AssetUUID {
        match self {
            Self::Asset(uuid) | Self::Made(uuid) => *uuid,
            Self::Local(source) => source.uuid(),
        }
    }

    /// Returns whether the reference names a value made in code, which only the session holds.
    pub fn is_made(&self) -> bool {
        matches!(self, Self::Made(_))
    }
}

impl<A: Asset> Clone for AssetRef<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Asset(uuid) => Self::Asset(*uuid),
            Self::Local(source) => Self::Local(source.clone()),
            Self::Made(uuid) => Self::Made(*uuid),
        }
    }
}

impl<A: Asset> PartialEq for AssetRef<A>
where
    A::LocalSource: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Asset(a), Self::Asset(b)) | (Self::Made(a), Self::Made(b)) => a == b,
            (Self::Local(a), Self::Local(b)) => a == b,
            _ => false,
        }
    }
}

impl<A: Asset> std::fmt::Debug for AssetRef<A>
where
    A::LocalSource: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asset(uuid) => f.debug_tuple("Asset").field(uuid).finish(),
            Self::Local(source) => f.debug_tuple("Local").field(source).finish(),
            Self::Made(uuid) => f.debug_tuple("Made").field(uuid).finish(),
        }
    }
}

impl<A: Asset> crate::ecs::Component for AssetRef<A> {}

#[cfg(test)]
mod tests;
