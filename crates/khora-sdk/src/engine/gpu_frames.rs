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

//! The frames whose reports wait for their GPU time to be read back.

use khora_core::agent::gorna::protocol::FrameReport;
use khora_core::renderer::api::device::{GpuFrameTime, RenderStats};
use std::collections::VecDeque;

/// How many frames a report waits for its GPU reading before it is sent with
/// the GPU unmeasured; readings arrive about two frames late.
const MAX_WAIT: u32 = 4;

/// Holds each GPU-timed frame's report until its own GPU time is read back,
/// so a report carries the time of the frame it describes, raw.
///
/// See the book: concepts/telemetry.md, "The graphics card".
#[derive(Debug, Default)]
pub(crate) struct GpuFrames {
    /// Whether the frame in progress was submitted timed on the GPU.
    timed: bool,
    /// The number the renderer gave the frame in progress when it presented.
    presented: Option<u64>,
    /// The latest GPU reading the renderer gave.
    read: Option<GpuFrameTime>,
    /// The timed frames waiting for their reading, oldest first.
    waiting: VecDeque<Waiting>,
}

impl GpuFrames {
    /// Starts a frame: nothing submitted or presented yet.
    pub(crate) fn opened(&mut self) {
        self.timed = false;
        self.presented = None;
    }

    /// Notes whether the frame in progress was submitted timed on the GPU.
    pub(crate) fn submitted(&mut self, timed: bool) {
        self.timed = timed;
    }

    /// Notes what the renderer said when the frame in progress presented: its
    /// number, and the latest GPU reading.
    pub(crate) fn presented(&mut self, stats: &RenderStats) {
        self.presented = Some(stats.frame_number);
        self.read = stats.gpu_frame;
    }

    /// Closes the frame in progress with `report`; returns the reports ready to
    /// send, each frame's GPU time its own reading.
    ///
    /// A timed frame waits for its reading; an untimed one is ready at once, the
    /// GPU unmeasured. A frame whose reading did not come within a few frames
    /// is sent unmeasured.
    pub(crate) fn close(&mut self, report: FrameReport) -> Vec<FrameReport> {
        let frame = self.presented.filter(|_| self.timed);
        self.opened();
        for waiting in &mut self.waiting {
            waiting.waited += 1;
        }
        self.waiting.push_back(Waiting {
            frame,
            report,
            waited: 0,
        });
        // A reading answers its frame, and every older one still waiting:
        // theirs will not come.
        if let Some(read) = self.read.take() {
            for waiting in &mut self.waiting {
                if let Some(frame) = waiting.frame.filter(|frame| *frame <= read.frame) {
                    if frame == read.frame {
                        waiting.report.busy.gpu = Some(read.time);
                    }
                    waiting.frame = None;
                }
            }
        }
        // In order: a frame answered waits behind an older one still waiting.
        let mut ready = Vec::new();
        while let Some(front) = self.waiting.front() {
            if front.frame.is_some() && front.waited <= MAX_WAIT {
                break;
            }
            ready.extend(self.waiting.pop_front().map(|waiting| waiting.report));
        }
        ready
    }
}

/// A frame's report, in the order the frames closed.
#[derive(Debug)]
struct Waiting {
    /// The frame's number, as the renderer gave it, while its reading is
    /// awaited; `None` once answered, or for a frame not timed.
    frame: Option<u64>,
    /// Its report, the GPU unmeasured so far.
    report: FrameReport,
    /// The frames closed since it closed.
    waited: u32,
}

#[cfg(test)]
mod tests {
    use super::{GpuFrames, MAX_WAIT};
    use khora_core::agent::gorna::protocol::{Epoch, FrameReport};
    use khora_core::lane::cost::PerDevice;
    use khora_core::renderer::api::device::{GpuFrameTime, RenderStats};
    use std::time::Duration;

    /// The report of a frame that kept the host busy `cpu_ms`, under `epoch`.
    fn report(cpu_ms: u64, epoch: u64) -> FrameReport {
        FrameReport {
            busy: PerDevice {
                cpu: Some(Duration::from_millis(cpu_ms)),
                ..PerDevice::default()
            },
            epoch: Epoch(epoch),
        }
    }

    /// Runs frame `frame`, timed, the renderer reading back `read` (frame, ms).
    fn timed_frame(
        frames: &mut GpuFrames,
        frame: u64,
        read: Option<(u64, u64)>,
        epoch: u64,
    ) -> Vec<FrameReport> {
        frames.submitted(true);
        frames.presented(&RenderStats {
            frame_number: frame,
            gpu_frame: read.map(|(frame, ms)| GpuFrameTime {
                frame,
                time: Duration::from_millis(ms),
            }),
            ..RenderStats::default()
        });
        frames.close(report(frame, epoch))
    }

    /// **An untimed frame reports at once, the GPU unmeasured.**
    #[test]
    fn an_untimed_frame_reports_at_once() {
        let mut frames = GpuFrames::default();
        frames.submitted(false);
        frames.presented(&RenderStats::default());

        let sent = frames.close(report(4, 0));

        assert_eq!(sent, vec![report(4, 0)]);
    }

    /// **A frame not presented reports at once.**
    #[test]
    fn a_frame_not_presented_reports_at_once() {
        let mut frames = GpuFrames::default();

        let sent = frames.close(report(4, 0));

        assert_eq!(sent, vec![report(4, 0)]);
    }

    /// **A timed frame waits for its own reading, and carries it.**
    ///
    /// Frame 1's time is read back as frame 3 presents: its report goes then,
    /// with the host time of frame 1 and the GPU time of frame 1.
    #[test]
    fn a_timed_frame_waits_for_its_own_reading() {
        let mut frames = GpuFrames::default();

        let first = timed_frame(&mut frames, 1, None, 7);
        let second = timed_frame(&mut frames, 2, None, 7);
        let third = timed_frame(&mut frames, 3, Some((1, 13)), 7);

        assert!(first.is_empty() && second.is_empty(), "waiting");
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].busy.cpu, Some(Duration::from_millis(1)));
        assert_eq!(third[0].busy.gpu, Some(Duration::from_millis(13)));
        assert_eq!(third[0].epoch, Epoch(7), "the epoch it ran under");
    }

    /// **A stall is one frame long in the reports.**
    ///
    /// Frame 2 keeps the GPU 120 ms; the renderer goes on returning its latest
    /// reading until the next one. Only frame 2's report carries 120 ms.
    #[test]
    fn a_stall_is_one_frame_long_in_the_reports() {
        let mut frames = GpuFrames::default();
        let mut sent = Vec::new();
        let reads = [
            None,
            None,
            Some((1, 13)),
            Some((2, 120)),
            Some((2, 120)),
            Some((4, 13)),
        ];
        for (index, read) in reads.into_iter().enumerate() {
            sent.extend(timed_frame(&mut frames, index as u64 + 1, read, 1));
        }

        let gpu: Vec<Option<Duration>> = sent.iter().map(|report| report.busy.gpu).collect();
        assert_eq!(
            gpu,
            vec![
                Some(Duration::from_millis(13)),
                Some(Duration::from_millis(120)),
                None,
                Some(Duration::from_millis(13)),
            ],
            "frame 3's reading never came: it goes unmeasured once frame 4's did"
        );
    }

    /// **An untimed frame closed behind a waiting one goes out after it.**
    ///
    /// The governor tells a hitch from load by the frame before it: the
    /// reports reach the DCC in the order the frames closed.
    #[test]
    fn an_untimed_frame_closed_behind_a_waiting_one_goes_out_after_it() {
        let mut frames = GpuFrames::default();
        let mut sent = timed_frame(&mut frames, 1, None, 0);
        frames.submitted(false);
        frames.presented(&RenderStats {
            frame_number: 2,
            ..RenderStats::default()
        });
        sent.extend(frames.close(report(2, 0)));
        sent.extend(timed_frame(&mut frames, 3, Some((1, 13)), 0));

        let order: Vec<Option<Duration>> = sent.iter().map(|report| report.busy.cpu).collect();
        assert_eq!(
            order,
            vec![
                Some(Duration::from_millis(1)),
                Some(Duration::from_millis(2))
            ],
            "frame 2, untimed, waits for frame 1's reading"
        );
    }

    /// **A reading that never comes leaves the frame unmeasured.**
    #[test]
    fn a_reading_that_never_comes_leaves_the_frame_unmeasured() {
        let mut frames = GpuFrames::default();
        let mut sent = timed_frame(&mut frames, 1, None, 0);
        for frame in 2..=u64::from(MAX_WAIT) + 2 {
            frames.submitted(false);
            frames.presented(&RenderStats {
                frame_number: frame,
                ..RenderStats::default()
            });
            sent.extend(frames.close(report(frame, 0)));
        }

        let first = (sent.iter())
            .find(|report| report.busy.cpu == Some(Duration::from_millis(1)))
            .expect("frame 1 is sent in the end");
        assert_eq!(first.busy.gpu, None);
    }
}
