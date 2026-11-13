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

//! Sub-assets: the containers that hold them, opened once and cached, the
//! parts extracted from them, and whether a part has left the project.

use std::any::Any;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use khora_core::asset::{Asset, AssetUUID};

use super::AssetService;
use crate::asset::container::AssetContainer;

impl AssetService {
    /// Whether the file `uuid` names can hold sub-assets (a glTF can): when
    /// its bytes change, its parts may have changed too, and the index must
    /// list them again.
    pub fn may_hold_sub_assets(&self, uuid: &AssetUUID) -> bool {
        self.reader
            .index
            .get_metadata(uuid)
            .is_some_and(|metadata| {
                crate::asset::container::type_may_hold_sub_assets(&metadata.asset_type_name)
            })
    }

    /// Registers the [`AssetContainer`] that opens files of index type
    /// `type_name` and extracts their sub-assets.
    pub fn register_container(&mut self, type_name: &str, container: impl AssetContainer) {
        self.reader_mut()
            .containers
            .insert(type_name.to_string(), Arc::new(container));
    }

    /// The container file `container` and what opens it, opened once and
    /// cached until it is invalidated. Each opening records the other files
    /// it reads.
    fn open_container(
        &mut self,
        container: AssetUUID,
    ) -> Result<(Arc<dyn AssetContainer>, Arc<dyn Any + Send + Sync>)> {
        let reader = Arc::clone(&self.reader);
        let opener = reader.opener_of(&container)?;
        if let Some(opened) = self.opened.get(&container) {
            return Ok((opener, opened.clone()));
        }
        let (opener, opened, files) = reader.open(&container)?;
        self.external_files.insert(container, files);
        self.opened.insert(container, opened.clone());
        Ok((opener, opened))
    }

    /// Extracts the sub-asset `label` (index type `type_name`) from the
    /// container file `container`, opening the container on first use.
    pub(super) fn extract_sub<A: Asset>(
        &mut self,
        container: AssetUUID,
        label: &str,
        type_name: &str,
    ) -> Result<A> {
        let (opener, opened) = self
            .open_container(container)
            .with_context(|| format!("sub-asset '{label}'"))?;
        let extracted =
            Arc::clone(&self.reader).extract(&opener, &opened, container, label, type_name)?;
        extracted.downcast::<A>().map(|asset| *asset).map_err(|_| {
            anyhow!(
                "sub-asset '{label}' of {container} is a '{type_name}', not a {}",
                std::any::type_name::<A>()
            )
        })
    }

    /// Whether `uuid` is no longer part of the project: it is not indexed
    /// and, for a sub-asset, its container is gone too, or opens and no
    /// longer holds it.
    ///
    /// A sub-asset whose container is indexed but does not open (a file
    /// caught mid-write, indexed without its parts) is not gone: its file
    /// is read again when it changes.
    pub fn is_gone(&mut self, uuid: &AssetUUID) -> bool {
        if self.gone_from_index(uuid) {
            return true;
        }
        if self.reader.index.get_metadata(uuid).is_some() {
            return false;
        }
        // A part whose container is still indexed: gone when the container
        // opens and no longer holds it.
        let container = self
            .served_subs
            .iter()
            .find(|(_, subs)| subs.contains(uuid))
            .map(|(container, _)| *container);
        container.is_some_and(|container| self.open_container(container).is_ok())
    }

    /// Whether the index alone says `uuid` left the project: it is not
    /// indexed and, for a part, its container is not indexed either. Reads no
    /// file — what decides a removed asset on the frame its change is seen.
    pub fn gone_from_index(&self, uuid: &AssetUUID) -> bool {
        if self.reader.index.get_metadata(uuid).is_some() {
            return false;
        }
        let container = self
            .served_subs
            .iter()
            .find(|(_, subs)| subs.contains(uuid))
            .map(|(container, _)| *container);
        match container {
            Some(container) => self.reader.index.get_metadata(&container).is_none(),
            None => true,
        }
    }
}
