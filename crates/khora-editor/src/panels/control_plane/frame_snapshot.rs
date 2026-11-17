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

//! What the summary bar shows of the last frame and the machine.

use khora_sdk::editor_ui::StatusBarData;
use khora_sdk::DccContext;

/// The last frame's numbers and the machine's load, as the summary bar shows them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FrameSnapshot {
    /// Frames per second.
    pub fps: f32,
    /// The last frame's time, in milliseconds.
    pub frame_time_ms: f32,
    /// Heap in use, in MB.
    pub heap_mb: f32,
    /// CPU load, `0.0..=1.0`.
    pub cpu_load: f32,
    /// GPU load, `0.0..=1.0`.
    pub gpu_load: f32,
    /// Video memory in use, in MB; 0 when not known.
    pub vram_mb: f32,
}

impl FrameSnapshot {
    /// Reads the editor's `status`, the DCC context's hardware numbers taking precedence for load and video memory.
    ///
    /// The probe the engine budgets with is the authority on them; its video
    /// memory counts only when it knows both the total and what is free.
    pub(super) fn read(status: Option<&StatusBarData>, dcc: Option<&DccContext>) -> Self {
        let mut snap = status.map_or_else(Self::default, |s| Self {
            fps: s.fps,
            frame_time_ms: s.frame_time_ms,
            heap_mb: s.memory_used_mb,
            cpu_load: s.cpu_load,
            gpu_load: s.gpu_load,
            vram_mb: s.vram_mb,
        });
        if let Some(ctx) = dcc {
            snap.cpu_load = ctx.hardware.cpu_load;
            snap.gpu_load = ctx.hardware.gpu_load;
            if let Some(vram_used) = ctx.hardware.available_vram.and_then(|avail| {
                ctx.hardware
                    .total_vram
                    .map(|total| (total.saturating_sub(avail)) as f32 / (1024.0 * 1024.0))
            }) {
                snap.vram_mb = vram_used;
            }
        }
        snap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> StatusBarData {
        StatusBarData {
            fps: 60.0,
            frame_time_ms: 16.0,
            memory_used_mb: 512.0,
            cpu_load: 0.1,
            gpu_load: 0.2,
            vram_mb: 300.0,
            ..StatusBarData::default()
        }
    }

    /// Without a DCC context, every number is the editor's.
    #[test]
    fn without_a_dcc_context_the_status_is_read() {
        assert_eq!(
            FrameSnapshot::read(Some(&status()), None),
            FrameSnapshot {
                fps: 60.0,
                frame_time_ms: 16.0,
                heap_mb: 512.0,
                cpu_load: 0.1,
                gpu_load: 0.2,
                vram_mb: 300.0,
            }
        );
    }

    /// The DCC's hardware numbers win for load and video memory; the frame and the heap stay the editor's.
    #[test]
    fn the_dcc_hardware_wins_for_load_and_video_memory() {
        let mut dcc = DccContext::default();
        dcc.hardware.cpu_load = 0.7;
        dcc.hardware.gpu_load = 0.8;
        dcc.hardware.total_vram = Some(4 * 1024 * 1024 * 1024);
        dcc.hardware.available_vram = Some(3 * 1024 * 1024 * 1024);

        let snap = FrameSnapshot::read(Some(&status()), Some(&dcc));

        assert_eq!(
            (snap.fps, snap.frame_time_ms, snap.heap_mb),
            (60.0, 16.0, 512.0)
        );
        assert_eq!((snap.cpu_load, snap.gpu_load), (0.7, 0.8));
        assert_eq!(snap.vram_mb, 1024.0);
    }

    /// A DCC that knows only part of the video memory leaves the editor's figure.
    #[test]
    fn a_partial_video_memory_reading_keeps_the_status_figure() {
        let mut dcc = DccContext::default();
        dcc.hardware.total_vram = Some(4 * 1024 * 1024 * 1024);
        dcc.hardware.available_vram = None;

        assert_eq!(
            FrameSnapshot::read(Some(&status()), Some(&dcc)).vram_mb,
            300.0
        );
    }

    /// Nothing to read reads zeros.
    #[test]
    fn nothing_to_read_reads_zeros() {
        assert_eq!(FrameSnapshot::read(None, None), FrameSnapshot::default());
    }
}
