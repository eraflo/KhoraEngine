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

//! What a lane builds its device resources with.

use std::marker::PhantomData;

use crate::renderer::GraphicsDevice;

/// The device a lane prepares and shuts down with.
///
/// A lane that needs no device (audio, physics, scripts) is prepared with
/// [`GpuSetup::headless`].
pub struct GpuSetup<'a> {
    device: Option<&'a dyn GraphicsDevice>,
    pipelines: Option<&'a dyn crate::renderer::traits::PipelineSystem>,
    borrows: PhantomData<&'a mut ()>,
}

impl<'a> GpuSetup<'a> {
    /// A setup on `device`.
    pub fn new(device: &'a dyn GraphicsDevice) -> Self {
        Self {
            device: Some(device),
            pipelines: None,
            borrows: PhantomData,
        }
    }

    /// A setup with no graphics device.
    pub fn headless() -> Self {
        Self {
            device: None,
            pipelines: None,
            borrows: PhantomData,
        }
    }

    /// The graphics device, when there is one.
    pub fn device(&self) -> Option<&'a dyn GraphicsDevice> {
        self.device
    }

    /// The same setup, building its pipelines and layouts through
    /// `pipelines`.
    pub fn with_pipelines(
        mut self,
        pipelines: &'a dyn crate::renderer::traits::PipelineSystem,
    ) -> Self {
        self.pipelines = Some(pipelines);
        self
    }

    /// The pipeline system, when there is one.
    pub fn pipelines(&self) -> Option<&'a dyn crate::renderer::traits::PipelineSystem> {
        self.pipelines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headless_setup_has_no_device() {
        assert!(GpuSetup::headless().device().is_none());
    }
}
