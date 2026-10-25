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

//! A flush that fails surfaces its error and still empties the queue: the
//! next frame draws its own text only, and the renderer recovers once the
//! device does.

use super::*;
use std::sync::atomic::AtomicBool;

/// A [`RecordingDevice`] whose shader-module creation and buffer writes fail
/// while `failing` is set.
#[derive(Debug, Default)]
struct FlakyDevice {
    inner: RecordingDevice,
    failing: AtomicBool,
}

impl FlakyDevice {
    fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::Relaxed);
    }

    fn fails(&self) -> bool {
        self.failing.load(Ordering::Relaxed)
    }
}

impl GraphicsDevice for FlakyDevice {
    fn create_shader_module(
        &self,
        d: &ShaderModuleDescriptor,
    ) -> Result<ShaderModuleId, ResourceError> {
        if self.fails() {
            return Err(ResourceError::BackendError("shader refused".to_owned()));
        }
        self.inner.create_shader_module(d)
    }
    fn destroy_shader_module(&self, id: ShaderModuleId) -> Result<(), ResourceError> {
        self.inner.destroy_shader_module(id)
    }
    fn create_render_pipeline(
        &self,
        d: &RenderPipelineDescriptor,
    ) -> Result<RenderPipelineId, ResourceError> {
        self.inner.create_render_pipeline(d)
    }
    fn create_pipeline_layout(
        &self,
        d: &PipelineLayoutDescriptor,
    ) -> Result<PipelineLayoutId, ResourceError> {
        self.inner.create_pipeline_layout(d)
    }
    fn destroy_render_pipeline(&self, id: RenderPipelineId) -> Result<(), ResourceError> {
        self.inner.destroy_render_pipeline(id)
    }
    fn create_compute_pipeline(
        &self,
        d: &ComputePipelineDescriptor,
    ) -> Result<ComputePipelineId, ResourceError> {
        self.inner.create_compute_pipeline(d)
    }
    fn destroy_compute_pipeline(&self, id: ComputePipelineId) -> Result<(), ResourceError> {
        self.inner.destroy_compute_pipeline(id)
    }
    fn create_bind_group_layout(
        &self,
        d: &BindGroupLayoutDescriptor,
    ) -> Result<BindGroupLayoutId, ResourceError> {
        self.inner.create_bind_group_layout(d)
    }
    fn create_bind_group(&self, d: &BindGroupDescriptor) -> Result<BindGroupId, ResourceError> {
        self.inner.create_bind_group(d)
    }
    fn destroy_bind_group_layout(&self, id: BindGroupLayoutId) -> Result<(), ResourceError> {
        self.inner.destroy_bind_group_layout(id)
    }
    fn destroy_bind_group(&self, id: BindGroupId) -> Result<(), ResourceError> {
        self.inner.destroy_bind_group(id)
    }
    fn create_buffer(&self, d: &BufferDescriptor) -> Result<BufferId, ResourceError> {
        self.inner.create_buffer(d)
    }
    fn create_buffer_with_data(
        &self,
        d: &BufferDescriptor,
        data: &[u8],
    ) -> Result<BufferId, ResourceError> {
        self.inner.create_buffer_with_data(d, data)
    }
    fn destroy_buffer(&self, id: BufferId) -> Result<(), ResourceError> {
        self.inner.destroy_buffer(id)
    }
    fn write_buffer(&self, id: BufferId, offset: u64, data: &[u8]) -> Result<(), ResourceError> {
        if self.fails() {
            return Err(ResourceError::OutOfBounds);
        }
        self.inner.write_buffer(id, offset, data)
    }
    fn write_buffer_async<'a>(
        &'a self,
        id: BufferId,
        offset: u64,
        data: &'a [u8],
    ) -> Box<dyn std::future::Future<Output = Result<(), ResourceError>> + Send + 'static> {
        self.inner.write_buffer_async(id, offset, data)
    }
    fn create_texture(&self, d: &TextureDescriptor) -> Result<TextureId, ResourceError> {
        self.inner.create_texture(d)
    }
    fn destroy_texture(&self, id: TextureId) -> Result<(), ResourceError> {
        self.inner.destroy_texture(id)
    }
    fn write_texture(
        &self,
        id: TextureId,
        data: &[u8],
        bpr: Option<u32>,
        offset: Origin3D,
        size: Extent3D,
    ) -> Result<(), ResourceError> {
        self.inner.write_texture(id, data, bpr, offset, size)
    }
    fn create_texture_view(
        &self,
        id: TextureId,
        d: &TextureViewDescriptor,
    ) -> Result<TextureViewId, ResourceError> {
        self.inner.create_texture_view(id, d)
    }
    fn destroy_texture_view(&self, id: TextureViewId) -> Result<(), ResourceError> {
        self.inner.destroy_texture_view(id)
    }
    fn create_sampler(&self, d: &SamplerDescriptor) -> Result<SamplerId, ResourceError> {
        self.inner.create_sampler(d)
    }
    fn destroy_sampler(&self, id: SamplerId) -> Result<(), ResourceError> {
        self.inner.destroy_sampler(id)
    }
    fn create_command_encoder(&self, label: Option<&str>) -> Box<dyn CommandEncoder> {
        self.inner.create_command_encoder(label)
    }
    fn submit_command_buffer(&self, cb: CommandBufferId) {
        self.inner.submit_command_buffer(cb)
    }
    fn get_surface_format(&self) -> Option<TextureFormat> {
        self.inner.get_surface_format()
    }
    fn get_surface_size(&self) -> (u32, u32) {
        self.inner.get_surface_size()
    }
    fn get_adapter_info(&self) -> GraphicsAdapterInfo {
        self.inner.get_adapter_info()
    }
    fn supports_feature(&self, feature: &str) -> bool {
        self.inner.supports_feature(feature)
    }
}

/// Flushes `renderer` into [`COLOR_TARGET`] at 800 x 600 and returns the
/// outcome and the passes the encoder recorded.
fn try_flush(
    renderer: &StandardTextRenderer,
    device: &FlakyDevice,
) -> (Result<(), String>, Vec<PassRecord>) {
    let passes = Passes::default();
    let mut encoder = RecordingEncoder {
        passes: passes.clone(),
    };
    let outcome = renderer
        .flush(device, &mut encoder, &COLOR_TARGET, extent(800, 600))
        .map_err(|e| e.to_string());
    let recorded = passes.lock().expect("passes lock").clone();
    (outcome, recorded)
}

#[test]
fn a_flush_whose_resources_cannot_be_created_fails_drops_its_text_and_recovers() {
    let renderer = renderer();
    let device = FlakyDevice::default();

    device.set_failing(true);
    queue(&renderer, "Lost", Vec2::new(0.0, 0.0));
    let (outcome, passes) = try_flush(&renderer, &device);
    assert!(outcome.is_err(), "the creation error surfaces");
    assert!(passes.is_empty(), "nothing is drawn");

    device.set_failing(false);
    queue(&renderer, "Hi", Vec2::new(0.0, 0.0));
    let (outcome, passes) = try_flush(&renderer, &device);
    assert_eq!(
        outcome,
        Ok(()),
        "the renderer builds its resources once the device can"
    );
    assert_eq!(
        indices_drawn(&passes),
        12,
        "only the text queued after the failure is drawn"
    );
}

#[test]
fn a_flush_whose_upload_fails_drops_its_text_for_the_next_frame() {
    let renderer = renderer();
    let device = FlakyDevice::default();
    queue(&renderer, "Warm", Vec2::new(0.0, 0.0));
    let (outcome, _) = try_flush(&renderer, &device);
    assert_eq!(outcome, Ok(()));

    device.set_failing(true);
    queue(&renderer, "Lost", Vec2::new(0.0, 0.0));
    let (outcome, passes) = try_flush(&renderer, &device);
    assert!(outcome.is_err(), "the upload error surfaces");
    assert!(passes.is_empty(), "nothing is drawn from a failed upload");

    device.set_failing(false);
    queue(&renderer, "Hi", Vec2::new(0.0, 0.0));
    let (outcome, passes) = try_flush(&renderer, &device);
    assert_eq!(outcome, Ok(()));
    assert_eq!(
        indices_drawn(&passes),
        12,
        "only the text queued after the failure is drawn"
    );
}
