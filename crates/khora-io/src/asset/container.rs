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

//! Sub-assets — assets that live inside another asset's file.
//!
//! A glTF file holds meshes, materials, textures and scenes. Each of them is
//! an asset of its own, with a uuid derived from the file's uuid and a label
//! ([`AssetUUID::sub_asset`]), and an [`khora_core::asset::AssetSource::Sub`] source
//! naming the file that holds it.
//!
//! Two pieces make a file type a container:
//! - a [`SubAssetListerRegistration`], which the index builder runs to list the
//!   sub-assets of a file of that type (reading metadata only);
//! - an [`AssetContainer`], registered on the
//!   [`AssetService`](super::AssetService), which opens the file once and
//!   extracts each sub-asset from it.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use khora_core::asset::AssetUUID;

pub use super::sub_asset_lister_registration::{
    type_may_hold_sub_assets, SubAssetEntry, SubAssetListerRegistration,
};

/// Opens a container file and extracts its sub-assets.
///
/// Registered on the [`AssetService`](super::AssetService) under the
/// container file's index type with
/// [`register_container`](super::AssetService::register_container).
pub trait AssetContainer: Send + Sync + 'static {
    /// Opens the container from its bytes.
    ///
    /// `source_path` is relative to the assets root, for files next to it. The
    /// opened value is cached per container uuid and shared by its sub-assets.
    fn open(&self, bytes: &[u8], source_path: &Path) -> anyhow::Result<Arc<dyn Any + Send + Sync>>;

    /// Opens the container within `limits`; by default, [`Self::open`], ignoring them.
    ///
    /// # Errors
    /// Holds a [`LimitExceeded`](crate::trust::LimitExceeded) when a limit stops it.
    fn open_limited(
        &self,
        bytes: &[u8],
        source_path: &Path,
        limits: &crate::trust::Limits,
    ) -> anyhow::Result<Arc<dyn Any + Send + Sync>> {
        let _ = limits;
        self.open(bytes, source_path)
    }

    /// Extracts the sub-asset `label` of index type `type_name` from an
    /// opened container, boxed as the concrete asset type for `type_name`.
    fn extract(
        &self,
        opened: &(dyn Any + Send + Sync),
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> anyhow::Result<Box<dyn Any + Send>>;

    /// Extracts the sub-asset `label` within `limits`; by default, [`Self::extract`].
    ///
    /// `limits` are those of the sub-asset's own type at the reader's trust.
    ///
    /// # Errors
    /// Holds a [`LimitExceeded`](crate::trust::LimitExceeded) when a limit stops it.
    fn extract_limited(
        &self,
        opened: &(dyn Any + Send + Sync),
        container: AssetUUID,
        label: &str,
        type_name: &str,
        limits: &crate::trust::Limits,
    ) -> anyhow::Result<Box<dyn Any + Send>> {
        let _ = limits;
        self.extract(opened, container, label, type_name)
    }

    /// Returns the files an opened container reads besides its own, as asset keys.
    ///
    /// Keys are forward-slash paths relative to the assets root: a glTF's external
    /// buffers and images. A change to one is a change to the container's
    /// sub-assets. None by default.
    fn external_files(&self, opened: &(dyn Any + Send + Sync)) -> Vec<String> {
        let _ = opened;
        Vec::new()
    }
}
