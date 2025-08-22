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

/// A thread-safe, reference-counted handle to a loaded asset.
///
/// This acts as a smart pointer, providing shared ownership of an asset's data.
/// Cloning a handle is cheap, as it only increments the reference count
/// and does not duplicate the underlying asset data.
///
/// The asset data is automatically deallocated when the last handle is dropped.
#[derive(Debug)]
pub struct AssetHandle<T: Asset>(Arc<T>);

impl<T: Asset> AssetHandle<T> {
    /// Creates a new `AssetHandle` that takes ownership of the asset data.
    ///
    /// This is typically called by the `AssetAgent` once an asset has been
    /// successfully loaded into memory.
    pub fn new(asset: T) -> Self {
        Self(Arc::new(asset))
    }
}

impl<T: Asset> Clone for AssetHandle<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Asset> Deref for AssetHandle<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}