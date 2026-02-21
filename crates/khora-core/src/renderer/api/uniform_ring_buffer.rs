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

//! Persistent ring buffer for GPU uniform data.
//!
//! The [`UniformRingBuffer`] eliminates per-frame GPU buffer allocation by pre-allocating
//! a set of buffers (one per frame-in-flight) and cycling through them. Each frame,
//! the next buffer slot is selected and its contents updated via `write_buffer()`,
//! ensuring the GPU can still read from a previous frame's data while the CPU writes
//! new data to the current slot.
//!
//! # Architecture
//!
//! ```text
//! Frame N:     [Slot 0: GPU reads] ← render pass uses this bind group
//! Frame N+1:   [Slot 1: CPU writes] → write_buffer() updates this slot
//! Frame N+2:   [Slot 0: CPU writes] → cycle back, GPU finished reading
//! ```
//!
//! Each slot has its own GPU buffer and pre-created bind group, avoiding both
//! buffer allocation and bind group creation during the render hot path.

use crate::renderer::{
    api::{
        bind_group::{
            BindGroupDescriptor, BindGroupEntry, BindGroupLayoutId, BindingResource, BufferBinding,
        },
        buffer::{BufferDescriptor, BufferId, BufferUsage},
        common::MAX_FRAMES_IN_FLIGHT,
    },
    error::ResourceError,
    traits::GraphicsDevice,
    BindGroupId,
};
use std::borrow::Cow;

/// A single slot in the ring buffer, holding a GPU buffer and its associated bind group.
#[derive(Debug)]
struct RingSlot {
    /// The persistent GPU buffer for this slot.
    buffer: BufferId,
    /// The pre-created bind group referencing this slot's buffer.
    bind_group: BindGroupId,
}

/// A persistent ring buffer for GPU uniform data that eliminates per-frame allocation.
///
/// Instead of creating and destroying GPU buffers every frame, the ring buffer
/// pre-allocates [`MAX_FRAMES_IN_FLIGHT`] buffer slots during initialization.
/// Each frame, [`advance()`](UniformRingBuffer::advance) moves to the next slot,
/// and [`write()`](UniformRingBuffer::write) updates the current slot's buffer contents.
///
/// The associated bind group for each slot is also pre-created, so the render pass
/// can simply call [`current_bind_group()`](UniformRingBuffer::current_bind_group)
/// without any per-frame resource creation.
///
/// # Example
///
/// ```ignore
/// // During initialization:
/// let ring = UniformRingBuffer::new(device, layout_id, 0, data_size, "Camera")?;
///
/// // Each frame:
/// ring.advance();
/// ring.write(device, bytemuck::bytes_of(&camera_uniforms))?;
/// let bind_group = ring.current_bind_group();
/// render_pass.set_bind_group(0, bind_group);
/// ```
#[derive(Debug)]
pub struct UniformRingBuffer {
    /// The ring buffer slots, one per frame-in-flight.
    slots: Vec<RingSlot>,
    /// The current slot index (cycles 0..MAX_FRAMES_IN_FLIGHT).
    current_index: usize,
    /// The size of the uniform data in bytes.
    data_size: u64,
    /// Debug label for logging.
    label: &'static str,
}

impl UniformRingBuffer {
    /// Creates a new `UniformRingBuffer` with pre-allocated GPU resources.
    ///
    /// This allocates [`MAX_FRAMES_IN_FLIGHT`] GPU buffers and creates a bind group
    /// for each one. The buffers are initialized with zeroed data.
    ///
    /// # Arguments
    ///
    /// * `device` - The graphics device to allocate GPU resources on.
    /// * `layout` - The bind group layout that the bind groups should conform to.
    /// * `binding` - The binding index within the bind group layout.
    /// * `data_size` - The size of the uniform data in bytes.
    /// * `label` - A debug label for the buffer (e.g., "Camera", "Lighting").
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] if buffer or bind group creation fails.
    pub fn new(
        device: &dyn GraphicsDevice,
        layout: BindGroupLayoutId,
        binding: u32,
        data_size: u64,
        label: &'static str,
    ) -> Result<Self, ResourceError> {
        let mut slots = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);

        for i in 0..MAX_FRAMES_IN_FLIGHT {
            let buffer_label = match i {
                0 => Cow::Borrowed(label),
                _ => Cow::Owned(format!("{label} [slot {i}]")),
            };

            let buffer = device.create_buffer(&BufferDescriptor {
                label: Some(buffer_label),
                size: data_size,
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
                        size: None,
                    }),
                    _phantom: std::marker::PhantomData,
                }],
            })?;

            slots.push(RingSlot { buffer, bind_group });
        }

        Ok(Self {
            slots,
            current_index: 0,
            data_size,
            label,
        })
    }

    /// Advances to the next slot in the ring buffer.
    ///
    /// This should be called once at the beginning of each frame, before writing
    /// new uniform data. It cycles through the slots so the GPU can still read
    /// from the previous frame's data.
    pub fn advance(&mut self) {
        self.current_index = (self.current_index + 1) % self.slots.len();
    }

    /// Writes uniform data to the current slot's GPU buffer.
    ///
    /// Uses `write_buffer()` to update the persistent buffer contents without
    /// reallocating. The data must be exactly `data_size` bytes.
    ///
    /// # Errors
    ///
    /// Returns a [`ResourceError`] if the GPU write fails.
    pub fn write(&self, device: &dyn GraphicsDevice, data: &[u8]) -> Result<(), ResourceError> {
        debug_assert_eq!(
            data.len() as u64,
            self.data_size,
            "UniformRingBuffer({}) write size mismatch: expected {}, got {}",
            self.label,
            self.data_size,
            data.len()
        );

        let slot = &self.slots[self.current_index];
        device.write_buffer(slot.buffer, 0, data)
    }

    /// Returns the bind group for the current slot.
    ///
    /// This bind group is pre-created and references the current slot's buffer.
    /// It can be passed directly to `set_bind_group()` on the render pass.
    pub fn current_bind_group(&self) -> &BindGroupId {
        &self.slots[self.current_index].bind_group
    }

    /// Returns the current slot index (for debugging/telemetry).
    pub fn current_slot_index(&self) -> usize {
        self.current_index
    }

    /// Returns the number of slots in the ring buffer.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Returns the data size per slot in bytes.
    pub fn data_size(&self) -> u64 {
        self.data_size
    }

    /// Destroys all GPU resources owned by this ring buffer.
    ///
    /// This must be called during shutdown to release GPU memory. After calling
    /// this method, the ring buffer should not be used again.
    pub fn destroy(&self, device: &dyn GraphicsDevice) {
        for slot in &self.slots {
            if let Err(e) = device.destroy_bind_group(slot.bind_group) {
                log::warn!(
                    "UniformRingBuffer({}): Failed to destroy bind group: {:?}",
                    self.label,
                    e
                );
            }
            if let Err(e) = device.destroy_buffer(slot.buffer) {
                log::warn!(
                    "UniformRingBuffer({}): Failed to destroy buffer: {:?}",
                    self.label,
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{
        api::{
            bind_group::{BindGroupLayoutDescriptor, BindGroupLayoutEntry},
            BindGroupLayoutId, BindingType, BufferBindingType, ComputePipelineDescriptor,
            PipelineLayoutDescriptor, RenderPipelineDescriptor, SamplerDescriptor,
            ShaderModuleDescriptor, ShaderStageFlags, TextureDescriptor, TextureViewDescriptor,
        },
        traits::{CommandEncoder, ComputePass, RenderPass},
        BindGroupId, ComputePipelineId, GraphicsAdapterInfo, GraphicsBackendType, IndexFormat,
        PipelineLayoutId, RenderPipelineId, RendererDeviceType, ResourceError, ShaderModuleId,
        TextureFormat,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A mock graphics device that produces unique resource IDs for testing.
    #[derive(Debug)]
    struct MockGraphicsDevice {
        next_id: AtomicUsize,
    }

    impl MockGraphicsDevice {
        fn new() -> Self {
            Self {
                next_id: AtomicUsize::new(1),
            }
        }

        fn next(&self) -> usize {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        }
    }

    struct MockCommandEncoder;
    struct MockRenderPass;
    struct MockComputePass;

    impl RenderPass<'_> for MockRenderPass {
        fn set_pipeline(&mut self, _p: &RenderPipelineId) {}
        fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
        fn set_vertex_buffer(&mut self, _s: u32, _b: &BufferId, _o: u64) {}
        fn set_index_buffer(&mut self, _b: &BufferId, _o: u64, _f: IndexFormat) {}
        fn draw(&mut self, _v: std::ops::Range<u32>, _i: std::ops::Range<u32>) {}
        fn draw_indexed(&mut self, _idx: std::ops::Range<u32>, _bv: i32, _i: std::ops::Range<u32>) {
        }
    }

    impl ComputePass<'_> for MockComputePass {
        fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
        fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
        fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
    }

    impl CommandEncoder for MockCommandEncoder {
        fn begin_render_pass<'enc>(
            &'enc mut self,
            _desc: &crate::renderer::api::command::RenderPassDescriptor<'enc>,
        ) -> Box<dyn RenderPass<'enc> + 'enc> {
            Box::new(MockRenderPass)
        }

        fn begin_compute_pass<'enc>(
            &'enc mut self,
            _desc: &crate::renderer::api::command::ComputePassDescriptor<'enc>,
        ) -> Box<dyn ComputePass<'enc> + 'enc> {
            Box::new(MockComputePass)
        }

        fn begin_profiler_compute_pass<'enc>(
            &'enc mut self,
            _label: Option<&str>,
            _profiler: &'enc dyn crate::renderer::traits::GpuProfiler,
            _pass_index: u32,
        ) -> Box<dyn ComputePass<'enc> + 'enc> {
            Box::new(MockComputePass)
        }

        fn copy_buffer_to_buffer(
            &mut self,
            _src: &BufferId,
            _src_off: u64,
            _dst: &BufferId,
            _dst_off: u64,
            _size: u64,
        ) {
        }

        fn finish(self: Box<Self>) -> crate::renderer::api::command::CommandBufferId {
            crate::renderer::api::command::CommandBufferId(0)
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    impl GraphicsDevice for MockGraphicsDevice {
        fn create_shader_module(
            &self,
            _d: &ShaderModuleDescriptor,
        ) -> Result<ShaderModuleId, ResourceError> {
            Ok(ShaderModuleId(self.next()))
        }
        fn destroy_shader_module(&self, _id: ShaderModuleId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_render_pipeline(
            &self,
            _d: &RenderPipelineDescriptor,
        ) -> Result<RenderPipelineId, ResourceError> {
            Ok(RenderPipelineId(self.next()))
        }
        fn create_pipeline_layout(
            &self,
            _d: &PipelineLayoutDescriptor,
        ) -> Result<PipelineLayoutId, ResourceError> {
            Ok(PipelineLayoutId(self.next()))
        }
        fn destroy_render_pipeline(&self, _id: RenderPipelineId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_compute_pipeline(
            &self,
            _d: &ComputePipelineDescriptor,
        ) -> Result<ComputePipelineId, ResourceError> {
            Ok(ComputePipelineId(self.next() as u64))
        }
        fn destroy_compute_pipeline(&self, _id: ComputePipelineId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_bind_group_layout(
            &self,
            _d: &BindGroupLayoutDescriptor,
        ) -> Result<BindGroupLayoutId, ResourceError> {
            Ok(BindGroupLayoutId(self.next()))
        }
        fn create_bind_group(
            &self,
            _d: &BindGroupDescriptor,
        ) -> Result<BindGroupId, ResourceError> {
            Ok(BindGroupId(self.next()))
        }
        fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn destroy_bind_group(&self, _id: BindGroupId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_buffer(&self, _d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
            Ok(BufferId(self.next()))
        }
        fn create_buffer_with_data(
            &self,
            _d: &BufferDescriptor,
            _data: &[u8],
        ) -> Result<BufferId, ResourceError> {
            Ok(BufferId(self.next()))
        }
        fn destroy_buffer(&self, _id: BufferId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn write_buffer(
            &self,
            _id: BufferId,
            _offset: u64,
            _data: &[u8],
        ) -> Result<(), ResourceError> {
            Ok(())
        }
        fn write_buffer_async<'a>(
            &'a self,
            _id: BufferId,
            _offset: u64,
            _data: &'a [u8],
        ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static>
        {
            Box::new(async { Ok(()) })
        }
        fn create_texture(
            &self,
            _d: &TextureDescriptor,
        ) -> Result<crate::renderer::TextureId, ResourceError> {
            Ok(crate::renderer::TextureId(self.next()))
        }
        fn destroy_texture(&self, _id: crate::renderer::TextureId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn write_texture(
            &self,
            _id: crate::renderer::TextureId,
            _data: &[u8],
            _bpr: Option<u32>,
            _offset: crate::math::dimension::Origin3D,
            _size: crate::math::dimension::Extent3D,
        ) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_texture_view(
            &self,
            _id: crate::renderer::TextureId,
            _d: &TextureViewDescriptor,
        ) -> Result<crate::renderer::TextureViewId, ResourceError> {
            Ok(crate::renderer::TextureViewId(self.next()))
        }
        fn destroy_texture_view(
            &self,
            _id: crate::renderer::TextureViewId,
        ) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_sampler(
            &self,
            _d: &SamplerDescriptor,
        ) -> Result<crate::renderer::SamplerId, ResourceError> {
            Ok(crate::renderer::SamplerId(self.next()))
        }
        fn destroy_sampler(&self, _id: crate::renderer::SamplerId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
            Box::new(MockCommandEncoder)
        }
        fn submit_command_buffer(&self, _cb: crate::renderer::api::command::CommandBufferId) {}
        fn get_surface_format(&self) -> Option<TextureFormat> {
            Some(TextureFormat::Rgba8UnormSrgb)
        }
        fn get_adapter_info(&self) -> GraphicsAdapterInfo {
            GraphicsAdapterInfo {
                name: "MockDevice".to_string(),
                backend_type: GraphicsBackendType::Unknown,
                device_type: RendererDeviceType::Unknown,
            }
        }
        fn supports_feature(&self, _feature: &str) -> bool {
            true
        }
    }

    fn create_test_layout(device: &MockGraphicsDevice) -> BindGroupLayoutId {
        device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("test_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX | ShaderStageFlags::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                }],
            })
            .expect("Failed to create test layout")
    }

    #[test]
    fn test_ring_buffer_creation() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let ring = UniformRingBuffer::new(&device, layout, 0, 256, "Test").unwrap();

        assert_eq!(ring.slot_count(), MAX_FRAMES_IN_FLIGHT);
        assert_eq!(ring.data_size(), 256);
        assert_eq!(ring.current_slot_index(), 0);
    }

    #[test]
    fn test_ring_buffer_advance_cycles() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let mut ring = UniformRingBuffer::new(&device, layout, 0, 64, "Test").unwrap();

        assert_eq!(ring.current_slot_index(), 0);
        ring.advance();
        assert_eq!(ring.current_slot_index(), 1);
        ring.advance();
        assert_eq!(ring.current_slot_index(), 0); // Wraps around
        ring.advance();
        assert_eq!(ring.current_slot_index(), 1);
    }

    #[test]
    fn test_ring_buffer_different_bind_groups_per_slot() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let mut ring = UniformRingBuffer::new(&device, layout, 0, 64, "Test").unwrap();

        let bg0 = *ring.current_bind_group();
        ring.advance();
        let bg1 = *ring.current_bind_group();

        // Each slot should have a distinct bind group
        assert_ne!(bg0, bg1, "Each slot should have a unique bind group");
    }

    #[test]
    fn test_ring_buffer_write() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let ring = UniformRingBuffer::new(&device, layout, 0, 16, "Test").unwrap();

        let data = [0u8; 16];
        let result = ring.write(&device, &data);
        assert!(result.is_ok(), "Write to ring buffer should succeed");
    }

    #[test]
    fn test_ring_buffer_write_after_advance() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let mut ring = UniformRingBuffer::new(&device, layout, 0, 16, "Test").unwrap();

        // Write to slot 0
        let data = [1u8; 16];
        assert!(ring.write(&device, &data).is_ok());

        // Advance and write to slot 1
        ring.advance();
        let data = [2u8; 16];
        assert!(ring.write(&device, &data).is_ok());

        // Advance back to slot 0 and write again
        ring.advance();
        let data = [3u8; 16];
        assert!(ring.write(&device, &data).is_ok());
    }

    #[test]
    fn test_ring_buffer_destroy() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let ring = UniformRingBuffer::new(&device, layout, 0, 64, "Test").unwrap();

        // Should not panic
        ring.destroy(&device);
    }

    #[test]
    fn test_ring_buffer_bind_group_cycles_back() {
        let device = MockGraphicsDevice::new();
        let layout = create_test_layout(&device);
        let mut ring = UniformRingBuffer::new(&device, layout, 0, 64, "Test").unwrap();

        let bg_initial = *ring.current_bind_group();

        // Cycle through all slots and back
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            ring.advance();
        }

        // Should be back to the same slot
        assert_eq!(
            *ring.current_bind_group(),
            bg_initial,
            "After full cycle, should return to initial bind group"
        );
    }
}
