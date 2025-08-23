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

use super::Asset;
use std::{ops::Deref, sync::Arc};

/// A thread-safe, reference-counted handle to a loaded asset's data.
///
/// This struct acts as a smart pointer that provides shared ownership of asset
/// data in memory. It is the primary way that game logic and systems should
/// interact with loaded assets.
///
/// Cloning an `AssetHandle` is a very cheap operation, as it only increments an
/// atomic reference counter. It does not duplicate the underlying asset data. The
/// asset data is automatically deallocated when the last handle pointing to it
/// is dropped.
///
/// This handle dereferences to `&T`, allowing for transparent, read-only access
/// to the asset's contents.
///
/// # Examples
///
/// ```
/// # use khora_core::asset::{Asset, AssetHandle};
/// # struct Texture {}
/// # impl Asset for Texture {}
/// let texture = Texture {};
///
/// // The AssetAgent creates the first handle when loading is complete.
/// let handle1 = AssetHandle::new(texture);
///
/// // Other systems can clone the handle to share access.
/// let handle2 = handle1.clone();
///
/// // Accessing the data is done via dereferencing (like with `Arc` or `Box`).
/// // let width = handle1.width; // Assuming Texture has a `width` field.
/// ```
#[derive(Debug)]
pub struct AssetHandle<T: Asset>(Arc<T>);

impl<T: Asset> AssetHandle<T> {
    /// Creates a new `AssetHandle` that takes ownership of the asset data.
    ///
    /// This is typically called by an asset loading system (like an `AssetAgent`)
    /// once an asset has been successfully loaded into memory.
    pub fn new(asset: T) -> Self {
        Self(Arc::new(asset))
    }
}

impl<T: Asset> Clone for AssetHandle<T> {
    /// Clones the handle, incrementing the reference count to the underlying asset.
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Asset> Deref for AssetHandle<T> {
    type Target = T;

    /// Provides transparent, immutable access to the underlying asset data.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
