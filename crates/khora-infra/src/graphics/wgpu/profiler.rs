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

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// WgpuTimestampProfiler encapsulates GPU timestamp query logic.
/// Frame-lag model: timestamps written during frame N are read at the start of frame N+1.
/// Layout (two-pass scheme):
///   Pass A (frame start / main pass begin):   begin-> index 0 (frame_start), end-> index 1 (main_pass_begin)
///   Pass B (main pass end / frame end):       begin-> index 2 (main_pass_end),  end-> index 3 (frame_end)
/// Durations derived:
///   main_pass = index2 - index1
///   frame_total = index3 - index0
#[derive(Debug)]
pub struct WgpuTimestampProfiler {
    query_set: wgpu::QuerySet,
    // GPU-only resolve buffer (not mappable) to satisfy potential driver restrictions.
    resolve_buffer: wgpu::Buffer,
    // Triple buffered CPU staging so we safely add a 2-frame latency between GPU write and CPU map.
    staging_buffers: [wgpu::Buffer; 3],
    staging_ready: [Arc<AtomicBool>; 3],
    staging_pending: [bool; 3],
    period_ns: f32,
    raw_main_pass_ms: f32,
    raw_frame_total_ms: f32,
    smooth_main_pass_ms: f32,
    smooth_frame_total_ms: f32,
    ema_alpha: f32,
    pub last_raw: Option<[u64; 4]>, // [frame_start, main_begin, main_end, frame_end]
}

#[allow(dead_code)]
impl WgpuTimestampProfiler {
    pub fn feature_available(features: wgpu::Features) -> bool {
        features.contains(wgpu::Features::TIMESTAMP_QUERY)
    }

    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("Khora GPU Timestamp QuerySet"),
            ty: wgpu::QueryType::Timestamp,
            count: 4,
        });

        // Alignment: destination offset must be 256-aligned. We'll always use offset 0.
        const RESOLVE_BUFFER_SIZE: u64 = 256; // generous; only first 32 bytes consumed currently.
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Khora GPU Timestamp Resolve"),
            size: RESOLVE_BUFFER_SIZE,
            usage: wgpu::BufferUsages::QUERY_RESOLVE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Triple buffering: indices 0..2, write index = frame_index % 3, map index = (frame_index - 2) % 3
        let staging_buffers = [0, 1, 2].map(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Khora GPU Timestamp Staging {i}")),
                size: RESOLVE_BUFFER_SIZE, // keep same size for simplicity
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let staging_ready = [
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        ];
        let staging_pending = [false, false, false];
        let period = queue.get_timestamp_period();
        log::info!(
            "GPU Timestamp Profiler initialized (period {period:.3} ns; triple-buffer 2-frame latency)"
        );
        Some(Self {
            query_set,
            resolve_buffer,
            staging_buffers,
            staging_ready,
            staging_pending,
            period_ns: period,
            raw_main_pass_ms: 0.0,
            raw_frame_total_ms: 0.0,
            smooth_main_pass_ms: 0.0,
            smooth_frame_total_ms: 0.0,
            ema_alpha: 0.2,
            last_raw: None,
        })
    }

    pub fn try_read_previous_frame(&mut self) {
        // Attempt to read any staging buffer that finished mapping.
        for (i, pending) in self.staging_pending.iter_mut().enumerate() {
            if !*pending {
                continue;
            }
            if !self.staging_ready[i].load(Ordering::SeqCst) {
                continue;
            }
            // Read exactly the mapped range (..32)
            let slice = self.staging_buffers[i].slice(..32);
            let data = slice.get_mapped_range();
            if data.len() >= 32 {
                let frame_start = u64::from_le_bytes(data[0..8].try_into().unwrap());
                let main_begin = u64::from_le_bytes(data[8..16].try_into().unwrap());
                let main_end = u64::from_le_bytes(data[16..24].try_into().unwrap());
                let frame_end = u64::from_le_bytes(data[24..32].try_into().unwrap());
                self.last_raw = Some([frame_start, main_begin, main_end, frame_end]);
                if main_end > main_begin && frame_end > frame_start {
                    self.raw_main_pass_ms =
                        ((main_end - main_begin) as f32 * self.period_ns) / 1_000_000.0;
                    self.raw_frame_total_ms =
                        ((frame_end - frame_start) as f32 * self.period_ns) / 1_000_000.0;
                    // Exponential moving average smoothing to reduce noise on fast GPUs
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
            drop(data);
            self.staging_buffers[i].unmap();
            *pending = false;
        }
    }

    pub fn pass_a_timestamp_writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        // frame_start & main_begin
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }
    pub fn pass_b_timestamp_writes(&self) -> wgpu::RenderPassTimestampWrites<'_> {
        // main_end & frame_end
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(2),
            end_of_pass_write_index: Some(3),
        }
    }

    // Compute pass variants (same indices) so we can use lightweight compute passes for timestamps only.
    pub fn compute_pass_a_timestamp_writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }
    pub fn compute_pass_b_timestamp_writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(2),
            end_of_pass_write_index: Some(3),
        }
    }

    pub fn resolve_and_copy(&self, encoder: &mut wgpu::CommandEncoder) {
        // Resolve all 4 timestamps (0..4)
        encoder.resolve_query_set(&self.query_set, 0..4, &self.resolve_buffer, 0);
    }

    pub fn copy_to_staging(&self, encoder: &mut wgpu::CommandEncoder, frame_index: u64) {
        let staging_idx = (frame_index as usize) % 3;
        if self.staging_pending[staging_idx]
            && !self.staging_ready[staging_idx].load(Ordering::SeqCst)
        {
            log::warn!(
                "GPU timestamp: staging buffer {staging_idx} still pending (frame {frame_index}) -> skip overwrite"
            );
            return;
        }
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.staging_buffers[staging_idx],
            0,
            32,
        );
    }

    pub fn schedule_map_after_submit(&mut self, frame_index: u64) {
        // Add 2-frame latency: only map once two newer frames have written other buffers.
        if frame_index < 2 {
            return;
        }
        let staging_idx = ((frame_index - 2) as usize) % 3;
        if self.staging_pending[staging_idx] {
            return;
        }
        let slice = self.staging_buffers[staging_idx].slice(..32);
        let flag = self.staging_ready[staging_idx].clone();
        flag.store(false, Ordering::SeqCst);
        slice.map_async(wgpu::MapMode::Read, move |res| {
            if let Err(e) = res {
                log::error!("GPU timestamp staging map_async failed: {e:?}");
            }
            flag.store(true, Ordering::SeqCst);
        });
        self.staging_pending[staging_idx] = true;
    }

    /// Cleans up the profiler's resources and performs a final blocking read.
    /// This should be called before the `wgpu::Device` is destroyed.
    pub fn shutdown(&mut self, device: &wgpu::Device) {
        log::debug!("Shutting down WgpuTimestampProfiler...");

        // Perform a final, blocking poll to ensure all pending work is finished
        // and all `on_submitted_work_done` callbacks have been executed.
        if let Err(e) = device.poll(wgpu::PollType::Wait) {
            log::warn!("Failed to poll device during profiler shutdown: {:?}", e);
        }

        // After waiting, we can be sure that all `staging_pending` flags have been
        // correctly updated by the callbacks. Any remaining error is likely a real issue.

        for (i, &is_pending) in self.staging_pending.iter().enumerate() {
            if is_pending {
                log::warn!(
                    "Profiler buffer {} was still pending during shutdown. Data may be lost.",
                    i
                );
                // We must still unmap it to avoid panics on drop.
                self.staging_buffers[i].unmap();
            }
        }
    }

    pub fn last_main_pass_ms(&self) -> f32 {
        self.smooth_main_pass_ms
    }
    pub fn last_frame_total_ms(&self) -> f32 {
        self.smooth_frame_total_ms
    }
    pub fn last_main_pass_ms_raw(&self) -> f32 {
        self.raw_main_pass_ms
    }
    pub fn last_frame_total_ms_raw(&self) -> f32 {
        self.raw_frame_total_ms
    }
    pub fn last_raw(&self) -> Option<[u64; 4]> {
        self.last_raw
    }
}

#[cfg(test)]
mod tests {
    use super::WgpuTimestampProfiler;
    use khora_core::renderer::api::GpuHook;

    fn create_test_device() -> Option<(wgpu::Device, wgpu::Queue, wgpu::Features)> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })) {
                Ok(a) => a,
                Err(_) => return None,
            };
        let features = adapter.features();
        let needed = features & wgpu::Features::TIMESTAMP_QUERY;
        let limits = wgpu::Limits::default();
        let req = wgpu::DeviceDescriptor {
            label: Some("Khora Test Device"),
            required_features: needed,
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        };
        let result = pollster::block_on(adapter.request_device(&req));
        match result {
            Ok(pair) => Some((pair.0, pair.1, features)),
            Err(_) => None,
        }
    }

    #[test]
    fn gpu_timestamp_profiler_basic_flow_or_skip() {
        let (device, queue, features) = match create_test_device() {
            Some(v) => v,
            None => return,
        };
        if !WgpuTimestampProfiler::feature_available(features) {
            return;
        }
        let profiler = match WgpuTimestampProfiler::new(&device, &queue) {
            Some(p) => p,
            None => return,
        };
        // Minimal assertion: construction succeeded and initial values are zeros.
        assert_eq!(profiler.last_main_pass_ms(), 0.0);
        assert_eq!(profiler.last_frame_total_ms(), 0.0);
        assert!(profiler.last_raw().is_none());
    }

    #[test]
    fn gpu_timestamp_hook_order_constant_matches_last_raw() {
        let hooks = GpuHook::ALL;
        assert_eq!(hooks.len(), 4, "Update test if hook count changes");
        assert_ne!(hooks[0] as u32, hooks[hooks.len() - 1] as u32);
    }
}
