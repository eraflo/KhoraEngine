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

use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindGroupLayoutId, BindingResource,
    BufferBinding,
};
use khora_core::renderer::api::frame::MAX_FRAMES_IN_FLIGHT;
use khora_core::renderer::api::resource::{BufferDescriptor, BufferId, BufferUsage};
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::traits::GraphicsDevice;
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

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::math::dimension::{Extent3D, Origin3D};
    use khora_core::renderer::api::command::{
        BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
        CommandBufferId, ComputePassDescriptor, RenderPassDescriptor,
    };
    use khora_core::renderer::api::device::{
        GraphicsAdapterInfo, GraphicsBackendType, RendererDeviceType,
    };
    use khora_core::renderer::api::pipeline::{
        ComputePipelineDescriptor, ComputePipelineId, PipelineLayoutDescriptor, PipelineLayoutId,
        RenderPipelineDescriptor, RenderPipelineId,
    };
    use khora_core::renderer::api::resource::texture::{
        SamplerDescriptor, SamplerId, TextureDescriptor, TextureId, TextureViewDescriptor,
        TextureViewId,
    };
    use khora_core::renderer::api::resource::{IndexFormat, TextureFormat};
    use khora_core::renderer::api::shader::{ShaderModuleDescriptor, ShaderModuleId};
    use khora_core::renderer::api::util::ShaderStageFlags;
    use khora_core::renderer::traits::{CommandEncoder, ComputePass, GpuProfiler, RenderPass};
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    /// A device that hands out unique IDs and records what the ring asks of it.
    #[derive(Debug, Default)]
    struct RecordingDevice {
        next_id: AtomicUsize,
        /// Every buffer created, with its size.
        buffers: Mutex<Vec<(BufferId, u64)>>,
        /// Every bind group created.
        bind_groups: Mutex<Vec<BindGroupId>>,
        /// Every write: target buffer, byte offset, bytes.
        writes: Mutex<Vec<(BufferId, u64, Vec<u8>)>>,
        destroyed_buffers: Mutex<Vec<BufferId>>,
        destroyed_bind_groups: Mutex<Vec<BindGroupId>>,
    }

    impl RecordingDevice {
        fn next(&self) -> usize {
            self.next_id.fetch_add(1, Ordering::Relaxed) + 1
        }

        fn last_write(&self) -> (BufferId, u64, Vec<u8>) {
            self.writes
                .lock()
                .expect("writes lock")
                .last()
                .cloned()
                .expect("at least one write")
        }

        fn buffer_count(&self) -> usize {
            self.buffers.lock().expect("buffers lock").len()
        }

        fn buffer_size(&self, id: BufferId) -> u64 {
            self.buffers
                .lock()
                .expect("buffers lock")
                .iter()
                .find(|(buffer, _)| *buffer == id)
                .map(|(_, size)| *size)
                .expect("buffer created by this device")
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
        fn set_viewport(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _min: f32, _max: f32) {}
        fn set_scissor_rect(&mut self, _x: u32, _y: u32, _w: u32, _h: u32) {}
    }

    impl ComputePass<'_> for MockComputePass {
        fn set_pipeline(&mut self, _p: &ComputePipelineId) {}
        fn set_bind_group(&mut self, _i: u32, _bg: &BindGroupId, _o: &[u32]) {}
        fn dispatch_workgroups(&mut self, _x: u32, _y: u32, _z: u32) {}
    }

    impl CommandEncoder for MockCommandEncoder {
        fn begin_render_pass<'enc>(
            &'enc mut self,
            _desc: &RenderPassDescriptor<'enc>,
        ) -> Box<dyn RenderPass<'enc> + 'enc> {
            Box::new(MockRenderPass)
        }

        fn begin_compute_pass<'enc>(
            &'enc mut self,
            _desc: &ComputePassDescriptor<'enc>,
        ) -> Box<dyn ComputePass<'enc> + 'enc> {
            Box::new(MockComputePass)
        }

        fn begin_profiler_compute_pass<'enc>(
            &'enc mut self,
            _label: Option<&str>,
            _profiler: &'enc dyn GpuProfiler,
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

        fn finish(self: Box<Self>) -> Option<CommandBufferId> {
            Some(CommandBufferId(0))
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    impl GraphicsDevice for RecordingDevice {
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
            let id = BindGroupId(self.next());
            self.bind_groups.lock().expect("bind groups lock").push(id);
            Ok(id)
        }
        fn destroy_bind_group_layout(&self, _id: BindGroupLayoutId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
            self.destroyed_bind_groups
                .lock()
                .expect("destroyed bind groups lock")
                .push(id);
            Ok(())
        }
        fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
            let id = BufferId(self.next());
            self.buffers
                .lock()
                .expect("buffers lock")
                .push((id, d.size));
            Ok(id)
        }
        fn create_buffer_with_data(
            &self,
            d: &BufferDescriptor,
            _data: &[u8],
        ) -> Result<BufferId, ResourceError> {
            self.create_buffer(d)
        }
        fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
            self.destroyed_buffers
                .lock()
                .expect("destroyed buffers lock")
                .push(id);
            Ok(())
        }
        fn write_buffer(
            &self,
            id: BufferId,
            offset: u64,
            data: &[u8],
        ) -> Result<(), ResourceError> {
            self.writes
                .lock()
                .expect("writes lock")
                .push((id, offset, data.to_vec()));
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
        fn create_texture(&self, _d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
            Ok(TextureId(self.next()))
        }
        fn destroy_texture(&self, _id: TextureId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn write_texture(
            &self,
            _id: TextureId,
            _data: &[u8],
            _bpr: Option<u32>,
            _offset: Origin3D,
            _size: Extent3D,
        ) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_texture_view(
            &self,
            _id: TextureId,
            _d: &TextureViewDescriptor,
        ) -> Result<TextureViewId, ResourceError> {
            Ok(TextureViewId(self.next()))
        }
        fn destroy_texture_view(&self, _id: TextureViewId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_sampler(&self, _d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
            Ok(SamplerId(self.next()))
        }
        fn destroy_sampler(&self, _id: SamplerId) -> Result<(), ResourceError> {
            Ok(())
        }
        fn create_command_encoder(&self, _label: Option<&str>) -> Box<dyn CommandEncoder> {
            Box::new(MockCommandEncoder)
        }
        fn submit_command_buffer(&self, _cb: CommandBufferId) {}
        fn get_surface_format(&self) -> Option<TextureFormat> {
            Some(TextureFormat::Rgba8UnormSrgb)
        }
        fn get_surface_size(&self) -> (u32, u32) {
            (1024, 1024)
        }
        fn get_adapter_info(&self) -> GraphicsAdapterInfo {
            GraphicsAdapterInfo {
                name: "RecordingDevice".to_string(),
                backend_type: GraphicsBackendType::Unknown,
                device_type: RendererDeviceType::Unknown,
            }
        }
        fn supports_feature(&self, _feature: &str) -> bool {
            true
        }
    }

    const ELEMENT: u32 = 64;
    const ALIGN: u32 = MIN_UNIFORM_ALIGNMENT;

    fn layout(device: &RecordingDevice) -> BindGroupLayoutId {
        device
            .create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("test_layout"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStageFlags::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: None,
                    },
                }],
            })
            .expect("layout")
    }

    fn ring(device: &RecordingDevice, max_elements: u32) -> DynamicUniformRingBuffer {
        let layout = layout(device);
        DynamicUniformRingBuffer::new(device, layout, 0, ELEMENT, max_elements, ALIGN, "Test")
            .expect("ring")
    }

    fn element(tag: u8) -> Vec<u8> {
        vec![tag; ELEMENT as usize]
    }

    /// Successive pushes land one aligned stride apart in the same buffer, and
    /// each push writes its bytes at the offset it returns — the dynamic
    /// offset a draw binds with.
    #[test]
    fn pushes_land_at_aligned_offsets_in_one_buffer() {
        let device = RecordingDevice::default();
        let mut ring = ring(&device, 8);

        let mut offsets = Vec::new();
        let mut targets = BTreeSet::new();
        for tag in 1..=3u8 {
            let offset = ring.push(&device, &element(tag)).expect("push");
            let (buffer, written_at, bytes) = device.last_write();
            assert_eq!(
                written_at, offset as u64,
                "bytes written where the offset says"
            );
            assert_eq!(bytes, element(tag));
            offsets.push(offset);
            targets.insert(buffer.0);
        }
        assert_eq!(offsets, vec![0, ALIGN, 2 * ALIGN]);
        assert_eq!(targets.len(), 1, "one chunk holds all three");
    }

    /// Each frame in flight writes its own buffer, starting from offset zero;
    /// after a full turn the first slot's buffer is written again from zero.
    #[test]
    fn advance_moves_to_the_next_slot_and_rewinds_it() {
        let device = RecordingDevice::default();
        let mut ring = ring(&device, 8);

        ring.push(&device, &element(1)).expect("push");
        ring.push(&device, &element(2)).expect("push");
        let (first_slot_buffer, _, _) = device.last_write();

        let mut slot_buffers = vec![first_slot_buffer];
        for step in 1..MAX_FRAMES_IN_FLIGHT {
            ring.advance();
            assert_eq!(ring.current_slot_index(), step);
            let offset = ring.push(&device, &element(3)).expect("push");
            assert_eq!(offset, 0, "a fresh slot starts at offset zero");
            let (buffer, _, _) = device.last_write();
            assert!(
                !slot_buffers.contains(&buffer),
                "slot {step} shares a buffer with an earlier slot"
            );
            slot_buffers.push(buffer);
        }

        ring.advance();
        assert_eq!(ring.current_slot_index(), 0, "the ring wraps");
        let offset = ring.push(&device, &element(4)).expect("push");
        assert_eq!(offset, 0, "a reused slot is rewound");
        assert_eq!(device.last_write().0, first_slot_buffer);
    }

    /// Pushing past a chunk's capacity grows the slot with a larger chunk and
    /// its own bind group; the next frame on that slot reuses the grown chunk
    /// rather than allocating yet another.
    #[test]
    fn overflow_grows_a_chunk_that_later_frames_reuse() {
        let device = RecordingDevice::default();
        let mut ring = ring(&device, 2);
        let created_by_new = device.buffer_count();

        ring.push(&device, &element(1)).expect("push");
        ring.push(&device, &element(2)).expect("push");
        let (first_chunk, _, _) = device.last_write();
        let first_bind_group = *ring.current_bind_group();

        let offset = ring.push(&device, &element(3)).expect("push");
        let (grown_chunk, written_at, bytes) = device.last_write();
        assert_eq!(offset, 0, "a new chunk starts at offset zero");
        assert_eq!(written_at, 0);
        assert_eq!(bytes, element(3));
        assert_ne!(
            grown_chunk, first_chunk,
            "the third element needs a new chunk"
        );
        assert!(
            device.buffer_size(grown_chunk) >= 2 * device.buffer_size(first_chunk),
            "the chunk grows at least twofold"
        );
        assert_ne!(
            *ring.current_bind_group(),
            first_bind_group,
            "the new chunk is bound through its own bind group"
        );
        assert_eq!(device.buffer_count(), created_by_new + 1);

        // A full turn later the same slot fills the same way.
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            ring.advance();
        }
        assert_eq!(ring.current_slot_index(), 0);
        for tag in 4..=6u8 {
            ring.push(&device, &element(tag)).expect("push");
        }
        assert_eq!(
            device.last_write().0,
            grown_chunk,
            "the grown chunk is reused"
        );
        assert_eq!(
            device.buffer_count(),
            created_by_new + 1,
            "no chunk allocated for a frame that fits"
        );
    }

    /// Destroying the ring releases every buffer and bind group it created,
    /// grown chunks included.
    #[test]
    fn destroy_releases_every_chunk() {
        let device = RecordingDevice::default();
        let mut ring = ring(&device, 1);
        ring.push(&device, &element(1)).expect("push");
        ring.push(&device, &element(2)).expect("push");

        ring.destroy(&device);

        let created: BTreeSet<usize> = device
            .buffers
            .lock()
            .expect("buffers lock")
            .iter()
            .map(|(id, _)| id.0)
            .collect();
        let destroyed: BTreeSet<usize> = device
            .destroyed_buffers
            .lock()
            .expect("destroyed buffers lock")
            .iter()
            .map(|id| id.0)
            .collect();
        assert_eq!(destroyed, created);

        let bound: BTreeSet<usize> = device
            .bind_groups
            .lock()
            .expect("bind groups lock")
            .iter()
            .map(|id| id.0)
            .collect();
        let unbound: BTreeSet<usize> = device
            .destroyed_bind_groups
            .lock()
            .expect("destroyed bind groups lock")
            .iter()
            .map(|id| id.0)
            .collect();
        assert_eq!(unbound, bound);
    }
}
