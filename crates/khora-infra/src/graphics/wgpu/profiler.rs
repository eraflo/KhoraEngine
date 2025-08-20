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

use crate::graphics::wgpu::command::WgpuCommandEncoder;
use crate::graphics::wgpu::device::WgpuDevice;
use khora_core::renderer::traits::{CommandEncoder, GpuProfiler};
use std::any::Any;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// WgpuTimestampProfiler encapsulates GPU timestamp query logic.
/// Frame-lag model: timestamps written during frame N are read at the start of frame N+2.
/// Layout (two-pass scheme):
///   Pass A (frame start / main pass begin):   begin-> index 0 (frame_start), end-> index 1 (main_pass_begin)
///   Pass B (main pass end / frame end):       begin-> index 2 (main_pass_end),  end-> index 3 (frame_end)
/// Durations derived:
///   main_pass = index2 - index1
///   frame_total = index3 - index0
#[derive(Debug)]
pub struct WgpuTimestampProfiler {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    staging_buffers: [wgpu::Buffer; 3],
    staging_ready: [Arc<AtomicBool>; 3],
    staging_pending: [bool; 3],
    period_ns: f32,
    raw_main_pass_ms: f32,
    raw_frame_total_ms: f32,
    smooth_main_pass_ms: f32,
    smooth_frame_total_ms: f32,
    ema_alpha: f32,
    last_raw: Option<[u64; 4]>,
}

impl WgpuTimestampProfiler {
    /// Checks if the required features for timestamp queries are available.
    pub fn feature_available(features: wgpu::Features) -> bool {
        features.contains(wgpu::Features::TIMESTAMP_QUERY)
    }

    /// Creates a new profiler. The `set_timestamp_period` method must be called
    /// afterwards with the period from the wgpu::Queue.
    pub fn new(device: &wgpu::Device) -> Option<Self> {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("Khora GPU Timestamp QuerySet"),
            ty: wgpu::QueryType::Timestamp,
            count: 4,
        });

        const RESOLVE_BUFFER_SIZE: u64 = 32; // 4 * u64 timestamps
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Khora GPU Timestamp Resolve Buffer"),
            size: RESOLVE_BUFFER_SIZE,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffers = [0, 1, 2].map(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Khora GPU Timestamp Staging Buffer {}", i)),
                size: RESOLVE_BUFFER_SIZE,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        let staging_ready = [
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ];

        Some(Self {
            query_set,
            resolve_buffer,
            staging_buffers,
            staging_ready,
            staging_pending: [false; 3],
            period_ns: 1.0, // Default value, must be updated via `set_timestamp_period`
            raw_main_pass_ms: 0.0,
            raw_frame_total_ms: 0.0,
            smooth_main_pass_ms: 0.0,
            smooth_frame_total_ms: 0.0,
            ema_alpha: 0.2,
            last_raw: None,
        })
    }

    /// Updates the timestamp period. This must be called after initialization.
    pub fn set_timestamp_period(&mut self, period_ns: f32) {
        self.period_ns = period_ns;
        log::info!("GPU Timestamp Profiler period set to {:.3} ns.", period_ns);
    }

    // These crate-public methods are implementation details for the WGPU backend.
    pub(crate) fn compute_pass_a_timestamp_writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    pub(crate) fn compute_pass_b_timestamp_writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(2),
            end_of_pass_write_index: Some(3),
        }
    }

    /// Cleans up the profiler's resources. This is crucial to prevent panics
    /// from dropping mapped buffers.
    pub fn shutdown(&mut self, device: &WgpuDevice) {
        log::debug!("Shutting down WgpuTimestampProfiler...");
        // This function will need access to the raw wgpu::Device to poll.
        // We'll add a helper method to WgpuDevice to expose this cleanly.
        device.poll_device_blocking();

        // After waiting, any pending callbacks should have completed.
        for (i, &is_pending) in self.staging_pending.iter().enumerate() {
            if is_pending {
                log::warn!(
                    "Profiler buffer {} was still pending during shutdown. Unmapping to prevent panic.",
                    i
                );
                // Unmapping a buffer that is not mapped is a no-op and safe.
                self.staging_buffers[i].unmap();
            }
        }
    }
}

impl GpuProfiler for WgpuTimestampProfiler {
    fn try_read_previous_frame(&mut self) {
        for i in 0..self.staging_pending.len() {
            if self.staging_pending[i] && self.staging_ready[i].load(Ordering::SeqCst) {
                let slice = self.staging_buffers[i].slice(..);
                // The buffer is mapped now, so we can get its contents.
                let data = slice.get_mapped_range();
                let timestamps: [u64; 4] = bytemuck::pod_read_unaligned(&data[..32]);
                drop(data); // Data guard is dropped, which is a prerequisite for unmapping.
                self.staging_buffers[i].unmap();
                self.staging_pending[i] = false;

                self.last_raw = Some(timestamps);
                let [frame_start, main_begin, main_end, frame_end] = timestamps;

                if main_end > main_begin && frame_end > frame_start {
                    self.raw_main_pass_ms =
                        ((main_end - main_begin) as f32 * self.period_ns) / 1_000_000.0;
                    self.raw_frame_total_ms =
                        ((frame_end - frame_start) as f32 * self.period_ns) / 1_000_000.0;

                    let a = self.ema_alpha;
                    self.smooth_main_pass_ms = if self.smooth_main_pass_ms == 0.0 {
                        self.raw_main_pass_ms
                    } else {
                        a * self.raw_main_pass_ms + (1.0 - a) * self.smooth_main_pass_ms
                    };
                    self.smooth_frame_total_ms = if self.smooth_frame_total_ms == 0.0 {
                        self.raw_frame_total_ms
                    } else {
                        a * self.raw_frame_total_ms + (1.0 - a) * self.smooth_frame_total_ms
                    };
                }
            }
        }
    }

    fn resolve_and_copy(&self, encoder: &mut dyn CommandEncoder) {
        let concrete_encoder = encoder
            .as_any_mut()
            .downcast_mut::<WgpuCommandEncoder>()
            .expect("Encoder must be a WgpuCommandEncoder for profiling");

        if let Some(wgpu_encoder) = concrete_encoder.wgpu_encoder_mut() {
            wgpu_encoder.resolve_query_set(&self.query_set, 0..4, &self.resolve_buffer, 0);
        }
    }

    fn copy_to_staging(&self, encoder: &mut dyn CommandEncoder, frame_index: u64) {
        let staging_idx = (frame_index as usize) % 3;

        if self.staging_pending[staging_idx]
            && !self.staging_ready[staging_idx].load(Ordering::SeqCst)
        {
            log::warn!(
                "GPU timestamp staging buffer {} is still pending, skipping overwrite.",
                staging_idx
            );
            return;
        }

        let concrete_encoder = encoder
            .as_any_mut()
            .downcast_mut::<WgpuCommandEncoder>()
            .expect("Encoder must be a WgpuCommandEncoder for profiling");

        if let Some(wgpu_encoder) = concrete_encoder.wgpu_encoder_mut() {
            wgpu_encoder.copy_buffer_to_buffer(
                &self.resolve_buffer,
                0,
                &self.staging_buffers[staging_idx],
                0,
                32,
            );
        }
    }

    fn schedule_map_after_submit(&mut self, frame_index: u64) {
        if frame_index < 2 {
            return;
        }
        let staging_idx = ((frame_index - 2) as usize) % 3;
        if self.staging_pending[staging_idx] {
            return;
        }

        let slice = self.staging_buffers[staging_idx].slice(..);
        let flag = self.staging_ready[staging_idx].clone();
        flag.store(false, Ordering::SeqCst);

        slice.map_async(wgpu::MapMode::Read, move |res| {
            if let Err(e) = res {
                log::error!("GPU timestamp staging map_async failed: {:?}", e);
            }
            flag.store(true, Ordering::SeqCst);
        });
        self.staging_pending[staging_idx] = true;
    }

    fn last_main_pass_ms(&self) -> f32 {
        self.smooth_main_pass_ms
    }

    fn last_frame_total_ms(&self) -> f32 {
        self.smooth_frame_total_ms
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::WgpuTimestampProfiler;
    use khora_core::renderer::traits::GpuProfiler;

    // Helper function to create a wgpu Device and Queue for testing purposes.
    // Returns None if a suitable adapter cannot be found.
    fn create_test_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::Features)> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;

        let features = adapter.features();
        let required_features = features & wgpu::Features::TIMESTAMP_QUERY;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Khora Test Device"),
            required_features,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .ok()?;

        Some((device, queue, features))
    }

    #[test]
    fn gpu_timestamp_profiler_initializes_correctly_or_skips() {
        // This test requires a physical device, so it might be skipped on CI without one.
        let (device, queue, features) = match create_test_device() {
            Some(v) => v,
            None => {
                println!("Skipping profiler test: could not create test device.");
                return;
            }
        };

        // Skip the test if the device does not support the required feature.
        if !WgpuTimestampProfiler::feature_available(features) {
            println!("Skipping profiler test: TIMESTAMP_QUERY feature not available.");
            return;
        }

        // 1. Create the profiler using the new signature.
        let mut profiler = WgpuTimestampProfiler::new(&device)
            .expect("Profiler creation should succeed when feature is available.");

        // 2. Set the timestamp period, which is now a separate step.
        let period = queue.get_timestamp_period();
        profiler.set_timestamp_period(period);

        // 3. Assert initial state using the public GpuProfiler trait methods.
        assert_eq!(profiler.last_main_pass_ms(), 0.0);
        assert_eq!(profiler.last_frame_total_ms(), 0.0);
    }
}
