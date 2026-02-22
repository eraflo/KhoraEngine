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

//! Persistent dynamic ring buffer for per-mesh or varying GPU uniform data.

use crate::renderer::{
    api::{
        command::{
            BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutId, BindingResource,
            BufferBinding,
        },
        core::MAX_FRAMES_IN_FLIGHT,
        resource::{BufferDescriptor, BufferId, BufferUsage},
    },
    error::ResourceError,
    traits::GraphicsDevice,
};
use std::borrow::Cow;

/// Default minimum uniform alignment required by most APIs
pub const MIN_UNIFORM_ALIGNMENT: u32 = 256;

/// Default capacity for a dynamic uniform buffer chunk.
pub const DEFAULT_MAX_ELEMENTS: u32 = 1000;

/// A chunk of memory representing a single buffer allocation within a slot.
#[derive(Debug)]
struct BufferChunk {
    buffer: BufferId,
    bind_group: BindGroupId,
    capacity: u32,
    current_offset: u32,
}

/// A single slot in the dynamic ring buffer spanning one frame in flight.
/// It contains multiple chunks that expand dynamically when capacity is reached.
#[derive(Debug)]
struct DynamicRingSlot {
    chunks: Vec<BufferChunk>,
    active_chunk_index: usize,
}

/// A persistent ring buffer for GPU uniform data that needs to be updated many times per frame.
/// Uses `has_dynamic_offset` to allow binding single elements within a larger uniform buffer.
/// Robustly allocates exponentially growing chunks when running out of space.
#[derive(Debug)]
pub struct DynamicUniformRingBuffer {
    slots: Vec<DynamicRingSlot>,
    current_index: usize,
    element_size: u32,
    alignment: u32,
    layout: BindGroupLayoutId,
    binding: u32,
    label: &'static str,
}

impl DynamicUniformRingBuffer {
    /// Creates a new dynamic uniform ring buffer.
    ///
    /// # Arguments
    ///
    /// * `device` - The graphics device to use.
    /// * `layout` - The bind group layout to use.
    /// * `binding` - The binding index to use.
    /// * `element_size` - The size of each element.
    /// * `max_elements` - The maximum number of elements.
    /// * `alignment` - The alignment of each element.
    /// * `label` - The label for the buffer.
    ///
    /// # Returns
    ///
    /// A `Result` containing the dynamic uniform ring buffer or a `ResourceError`.
    pub fn new(
        device: &dyn GraphicsDevice,
        layout: BindGroupLayoutId,
        binding: u32,
        element_size: u32,
        max_elements: u32,
        alignment: u32,
        label: &'static str,
    ) -> Result<Self, ResourceError> {
        let mut slots = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);

        let aligned_element_size = (element_size + alignment - 1) & !(alignment - 1);
        let initial_capacity = aligned_element_size * max_elements;

        for i in 0..MAX_FRAMES_IN_FLIGHT {
            let buffer_label = match i {
                0 => Cow::Borrowed(label),
                _ => Cow::Owned(format!("{} [slot {}]", label, i)),
            };

            let buffer = device.create_buffer(&BufferDescriptor {
                label: Some(buffer_label),
                size: initial_capacity as u64,
                usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                mapped_at_creation: false,
            })?;

            let bind_group = device.create_bind_group(&BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[BindGroupEntry {
                    binding,
                    resource: BindingResource::Buffer(BufferBinding {
                        buffer,
                        offset: 0,
                        size: std::num::NonZeroU64::new(element_size as u64),
                    }),
                    _phantom: std::marker::PhantomData,
                }],
            })?;

            slots.push(DynamicRingSlot {
                chunks: vec![BufferChunk {
                    buffer,
                    bind_group,
                    capacity: initial_capacity,
                    current_offset: 0,
                }],
                active_chunk_index: 0,
            });
        }

        Ok(Self {
            slots,
            current_index: 0,
            element_size,
            alignment,
            layout,
            binding,
            label,
        })
    }

    /// Advances the ring buffer to the next frame.
    pub fn advance(&mut self) {
        self.current_index = (self.current_index + 1) % self.slots.len();
        let slot = &mut self.slots[self.current_index];
        for chunk in &mut slot.chunks {
            chunk.current_offset = 0;
        }
        slot.active_chunk_index = 0;
    }

    /// Pushes data to the current buffer and returns the dynamic offset.
    pub fn push(&mut self, device: &dyn GraphicsDevice, data: &[u8]) -> Result<u32, ResourceError> {
        let aligned_size = (data.len() as u32 + self.alignment - 1) & !(self.alignment - 1);
        let slot = &mut self.slots[self.current_index];

        // Handle auto-allocation out of capacity bounds
        if slot.chunks[slot.active_chunk_index].current_offset + aligned_size
            > slot.chunks[slot.active_chunk_index].capacity
        {
            let mut chunk_found = false;

            // Check if the next chunk has enough capacity
            if slot.active_chunk_index + 1 < slot.chunks.len()
                && slot.chunks[slot.active_chunk_index + 1].capacity >= aligned_size
            {
                slot.active_chunk_index += 1;
                chunk_found = true;
            }

            // If no chunk is found, create a new one
            if !chunk_found {
                let current_capacity = slot.chunks[slot.active_chunk_index].capacity;
                let new_capacity = (current_capacity * 2).max(aligned_size * 100);
                let chunk_idx = slot.chunks.len();

                let buffer_label = Cow::Owned(format!(
                    "{} [slot {} chunk {}]",
                    self.label, self.current_index, chunk_idx
                ));
                let buffer = device.create_buffer(&BufferDescriptor {
                    label: Some(buffer_label),
                    size: new_capacity as u64,
                    usage: BufferUsage::UNIFORM | BufferUsage::COPY_DST,
                    mapped_at_creation: false,
                })?;

                let bind_group = device.create_bind_group(&BindGroupDescriptor {
                    label: Some(self.label),
                    layout: self.layout,
                    entries: &[BindGroupEntry {
                        binding: self.binding,
                        resource: BindingResource::Buffer(BufferBinding {
                            buffer,
                            offset: 0,
                            size: std::num::NonZeroU64::new(self.element_size as u64),
                        }),
                        _phantom: std::marker::PhantomData,
                    }],
                })?;

                slot.chunks.push(BufferChunk {
                    buffer,
                    bind_group,
                    capacity: new_capacity,
                    current_offset: 0,
                });
                slot.active_chunk_index = chunk_idx;
            }
        }

        let chunk = &mut slot.chunks[slot.active_chunk_index];
        let offset = chunk.current_offset;
        device.write_buffer(chunk.buffer, offset as u64, data)?;

        chunk.current_offset += aligned_size;

        Ok(offset)
    }

    /// Returns the current bind group.
    pub fn current_bind_group(&self) -> &BindGroupId {
        let slot = &self.slots[self.current_index];
        &slot.chunks[slot.active_chunk_index].bind_group
    }

    /// Returns the current slot index.
    pub fn current_slot_index(&self) -> usize {
        self.current_index
    }

    /// Destroys the dynamic uniform ring buffer.
    pub fn destroy(&self, device: &dyn GraphicsDevice) {
        for slot in &self.slots {
            for chunk in &slot.chunks {
                if let Err(e) = device.destroy_bind_group(chunk.bind_group) {
                    log::warn!(
                        "DynamicUniformRingBuffer({}): Failed to destroy bind group: {:?}",
                        self.label,
                        e
                    );
                }
                if let Err(e) = device.destroy_buffer(chunk.buffer) {
                    log::warn!(
                        "DynamicUniformRingBuffer({}): Failed to destroy buffer: {:?}",
                        self.label,
                        e
                    );
                }
            }
        }
    }
}
