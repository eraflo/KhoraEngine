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

//! What the overlay lanes build alike: a buffer bound alone in a bind group,
//! and the camera block every overlay is seen through.

use std::borrow::Cow;

use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutId, BindingResource,
    BufferBinding,
};
use khora_core::renderer::api::resource::{
    BufferDescriptor, BufferId, BufferUsage, CameraUniformData,
};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::GraphicsDevice;
use khora_data::render::ExtractedView;

/// A buffer and the bind group that binds it whole at binding 0.
#[derive(Debug, Clone, Copy)]
pub(super) struct Block {
    /// The buffer.
    pub buffer: BufferId,
    /// The bind group over it.
    pub bind_group: BindGroupId,
    /// Its size, in bytes.
    pub size: u64,
}

impl Block {
    /// A buffer of `size` bytes with `usage`, bound by a bind group of
    /// `layout`. A refused bind group gives the buffer back, so a refusal
    /// leaves nothing behind.
    pub fn new(
        device: &dyn GraphicsDevice,
        layout: BindGroupLayoutId,
        size: u64,
        usage: BufferUsage,
        label: &'static str,
    ) -> Result<Self, ResourceError> {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some(Cow::Borrowed(label)),
            size,
            usage: usage | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        })?;
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::Buffer(BufferBinding {
                    buffer,
                    offset: 0,
                    size: None,
                }),
                _phantom: std::marker::PhantomData,
            }],
        });
        match bind_group {
            Ok(bind_group) => Ok(Self {
                buffer,
                bind_group,
                size,
            }),
            Err(error) => {
                if let Err(e) = device.destroy_buffer(buffer) {
                    log::warn!("{label}: could not destroy the buffer: {e:?}");
                }
                Err(error)
            }
        }
    }

    /// A uniform block holding one camera.
    pub fn camera(
        device: &dyn GraphicsDevice,
        layout: BindGroupLayoutId,
        label: &'static str,
    ) -> Result<Self, ResourceError> {
        Self::new(
            device,
            layout,
            std::mem::size_of::<CameraUniformData>() as u64,
            BufferUsage::UNIFORM,
            label,
        )
    }

    /// Writes `view`'s view-projection and position into the block.
    pub fn write_camera(
        &self,
        device: &dyn GraphicsDevice,
        view: &ExtractedView,
    ) -> Result<(), ResourceError> {
        let camera = CameraUniformData {
            view_projection: view.view_proj.to_cols_array_2d(),
            camera_position: [view.position.x, view.position.y, view.position.z, 1.0],
        };
        device.write_buffer(self.buffer, 0, bytemuck::bytes_of(&camera))
    }

    /// Gives the bind group and the buffer back.
    pub fn destroy(self, device: &dyn GraphicsDevice) {
        if let Err(e) = device.destroy_bind_group(self.bind_group) {
            log::warn!("overlay: could not destroy a bind group: {e:?}");
        }
        if let Err(e) = device.destroy_buffer(self.buffer) {
            log::warn!("overlay: could not destroy a buffer: {e:?}");
        }
    }
}
