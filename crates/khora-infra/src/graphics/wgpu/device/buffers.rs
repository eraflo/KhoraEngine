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

//! The wgpu device's buffers: allocating them (with the VRAM they count),
//! writing to them through the queue or a mapping, releasing them.

use khora_core::renderer::api::resource::buffer::{self as api_buf};
use khora_core::renderer::ResourceError;

use crate::graphics::wgpu::conversions::IntoWgpu;

use std::future::Future;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use wgpu;
use wgpu::util::DeviceExt;

use super::entries::WgpuBufferEntry;
use super::map_async::{MapAsyncFutureState, MapAsyncOperationFuture};
use super::{buffer_descriptor, WgpuDevice};

impl WgpuDevice {
    /// Allocates an empty buffer, counts its size as VRAM, and registers it
    /// under a new id.
    pub(super) fn allocate_buffer(
        &self,
        descriptor: &api_buf::BufferDescriptor,
    ) -> Result<api_buf::BufferId, ResourceError> {
        let wgpu_buffer_descriptor = buffer_descriptor(descriptor);
        let wgpu_buffer = self.wgpu_device().create_buffer(&wgpu_buffer_descriptor);
        let id = self.generate_buffer_id();

        // Track VRAM usage
        self.internal
            .vram_allocated_bytes
            .fetch_add(descriptor.size as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Insert the buffer into the map
        self.with_registry(|r| {
            r.buffers.insert(
                id,
                WgpuBufferEntry {
                    wgpu_buffer: Arc::new(wgpu_buffer),
                    size: descriptor.size,
                },
            )
        })?;

        log::debug!(
            "WgpuDevice: Created buffer '{:?}' with ID: {:?}, size: {} bytes",
            descriptor
                .label
                .as_ref()
                .map(|s: &std::borrow::Cow<'_, str>| s.as_ref())
                .unwrap_or_default(),
            id,
            descriptor.size
        );
        Ok(id)
    }

    /// Allocates a buffer holding `data`, counts its size as VRAM, and
    /// registers it under a new id.
    pub(super) fn allocate_buffer_with_data(
        &self,
        descriptor: &api_buf::BufferDescriptor,
        data: &[u8],
    ) -> Result<api_buf::BufferId, ResourceError> {
        let wgpu_buffer =
            self.wgpu_device()
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: descriptor.label.as_deref(),
                    contents: data,
                    usage: descriptor.usage.into_wgpu(),
                });

        // Use the existing ID generation and storage system.
        let id = self.generate_buffer_id();
        let buffer_size = data.len() as u64;

        // Track VRAM usage
        self.internal
            .vram_allocated_bytes
            .fetch_add(buffer_size as usize, Ordering::Relaxed);
        let current_vram = self.internal.vram_allocated_bytes.load(Ordering::Relaxed) as u64;
        self.internal
            .vram_peak_bytes
            .fetch_max(current_vram, Ordering::Relaxed);

        // Store the buffer
        self.with_registry(|r| {
            r.buffers.insert(
                id,
                WgpuBufferEntry {
                    wgpu_buffer: Arc::new(wgpu_buffer),
                    size: buffer_size,
                },
            )
        })?;

        log::debug!(
            "WgpuDevice: Created buffer '{:?}' with initial data. ID: {:?}, size: {} bytes",
            descriptor.label.as_deref().unwrap_or_default(),
            id,
            buffer_size
        );
        Ok(id)
    }

    /// Removes a buffer from the registry and gives its size back to the
    /// VRAM count.
    pub(super) fn release_buffer(&self, id: api_buf::BufferId) -> Result<(), ResourceError> {
        // Remove the buffer from the map and track VRAM usage
        if let Some(entry) = self.with_registry(|r| r.buffers.remove(&id))? {
            self.internal
                .vram_allocated_bytes
                .fetch_sub(entry.size as usize, Ordering::Relaxed);
            log::debug!("WgpuDevice: Destroyed buffer with ID: {id:?}");
            Ok(())
        } else {
            Err(ResourceError::NotFound)
        }
    }

    /// Queues a write of `data` into a buffer at `offset`, refusing one that
    /// would run past its end.
    pub(super) fn queue_buffer_write(
        &self,
        id: api_buf::BufferId,
        offset: u64,
        data: &[u8],
    ) -> Result<(), ResourceError> {
        // The buffer, cloned out of the registry.
        let wgpu_buffer = self
            .with_registry(|r| r.buffers.get(&id).map(|e| Arc::clone(&e.wgpu_buffer)))?
            .ok_or(ResourceError::NotFound)?;

        // Check the bounds
        let buffer_size = wgpu_buffer.size();
        let end_offset = offset + data.len() as u64;
        if end_offset > buffer_size {
            return Err(ResourceError::OutOfBounds);
        }

        // Write directly. No padding, no allocation. It's simple and efficient.
        self.wgpu_queue().write_buffer(&wgpu_buffer, offset, data);

        log::debug!(
            "WgpuDevice: Wrote {} bytes to buffer ID: {:?} at offset {}",
            data.len(),
            id,
            offset
        );

        Ok(())
    }

    /// Maps a range of a buffer for writing and copies `data` into it once
    /// the mapping lands; the future resolves when the copy is done.
    pub(super) fn map_buffer_write(
        &self,
        id: api_buf::BufferId,
        offset: u64,
        data: &[u8],
    ) -> Box<dyn Future<Output = Result<(), ResourceError>> + Send + 'static> {
        let entry_wgpu_buffer: Arc<wgpu::Buffer> =
            match self.with_registry(|r| r.buffers.get(&id).map(|e| Arc::clone(&e.wgpu_buffer))) {
                Ok(Some(buffer)) => buffer,
                Ok(None) => return Box::new(async { Err(ResourceError::NotFound) }),
                Err(error) => return Box::new(async { Err(error) }),
            };

        // Data needs to be owned by the future to be 'static.
        // This involves a copy, which is a common trade-off for true async operations.
        let owned_data = data.to_vec(); // One copy, owned by the future

        // Create a shared state for the Future and the WGPU callback
        let shared_state = Arc::new(MapAsyncFutureState {
            result: Mutex::new(None),
            waker: Mutex::new(None),
        });

        // Clones and moves for the `map_async` callback's `move` closure
        let future_state_for_callback = Arc::clone(&shared_state);
        let buffer_id_for_callback = id;
        let entry_wgpu_buffer_for_callback = Arc::clone(&entry_wgpu_buffer);
        let owned_data_for_callback = owned_data;

        // Schedule the WGPU map_async operation
        let buffer_slice =
            entry_wgpu_buffer.slice(offset..(offset + owned_data_for_callback.len() as u64));

        buffer_slice.map_async(wgpu::MapMode::Write, move |result| {
            // This closure runs on WGPU's internal callback thread/executor
            let final_result = if let Err(e) = result {
                log::error!(
                    "Failed to map buffer asynchronously for ID {buffer_id_for_callback:?}: {e:?}"
                );
                Err(ResourceError::BackendError(format!(
                    "WGPU map_async failed: {e:?}"
                )))
            } else {
                // Mapping was successful. Now perform the actual data copy.
                // Re-slice the buffer from the entry_wgpu_buffer_for_callback as the original buffer_slice
                // would have been dropped or moved.
                let buffer_slice_for_copy = entry_wgpu_buffer_for_callback
                    .slice(offset..(offset + owned_data_for_callback.len() as u64));
                let mut mapped_range = buffer_slice_for_copy.get_mapped_range_mut();
                mapped_range.copy_from_slice(&owned_data_for_callback); // This is the actual data write
                drop(mapped_range); // Explicitly unmap the buffer by dropping the guard
                entry_wgpu_buffer_for_callback.unmap(); // Ensure WGPU knows it's unmapped
                log::debug!(
                    "WgpuDevice: Async map_async copy complete for buffer ID: {buffer_id_for_callback:?}"
                );
                Ok(())
            };

            // Signal the Future that the operation is complete. If the
            // mutex is poisoned the future is unreachable anyway — log and
            // give up rather than panic on the callback thread.
            match future_state_for_callback.result.lock() {
                Ok(mut g) => *g = Some(final_result),
                Err(_) => {
                    log::error!(
                        "write_buffer_async callback for {buffer_id_for_callback:?}: \
                         result mutex poisoned"
                    );
                    return;
                }
            }

            // Wake the Future if a waker was stored.
            match future_state_for_callback.waker.lock() {
                Ok(mut g) => {
                    if let Some(waker) = g.take() {
                        waker.wake();
                    }
                }
                Err(_) => {
                    log::error!(
                        "write_buffer_async callback for {buffer_id_for_callback:?}: \
                         waker mutex poisoned"
                    );
                }
            }
        });

        // Return a Future that will wait for the callback to complete
        Box::new(MapAsyncOperationFuture {
            state: shared_state,
        })
    }
}
