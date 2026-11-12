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

//! Bind groups kept while what they bind holds.

use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::GraphicsDevice;

/// Bind groups keyed by what they bind, at most `capacity` of them: one is
/// made the first time its key is seen and kept while the key comes back;
/// past the capacity the oldest is given back to the device.
///
/// A lane whose uniforms rotate through a ring of buffers keeps one per
/// buffer of the ring.
#[derive(Debug)]
pub(crate) struct KeptBindGroups<K> {
    capacity: usize,
    kept: Vec<(K, BindGroupId)>,
}

impl<K: PartialEq> KeptBindGroups<K> {
    /// No bind group yet, holding at most `capacity`.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            kept: Vec::new(),
        }
    }

    /// The bind group of `key`: the one kept, else the one `make` creates.
    pub fn get(
        &mut self,
        key: K,
        device: &dyn GraphicsDevice,
        make: impl FnOnce() -> Result<BindGroupId, ResourceError>,
    ) -> Result<BindGroupId, ResourceError> {
        if let Some((_, group)) = self.kept.iter().find(|(kept, _)| *kept == key) {
            return Ok(*group);
        }
        let group = make()?;
        if self.kept.len() == self.capacity {
            let (_, oldest) = self.kept.remove(0);
            if let Err(error) = device.destroy_bind_group(oldest) {
                log::warn!("a kept bind group could not be destroyed: {error:?}");
            }
        }
        self.kept.push((key, group));
        Ok(group)
    }

    /// Gives every kept bind group back to the device.
    pub fn destroy(&mut self, device: &dyn GraphicsDevice) {
        for (_, group) in self.kept.drain(..) {
            if let Err(error) = device.destroy_bind_group(group) {
                log::warn!("a kept bind group could not be destroyed: {error:?}");
            }
        }
    }
}
