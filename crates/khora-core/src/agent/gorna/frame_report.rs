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

//! What one frame cost the engine, per device.

use std::time::Duration;

use crate::lane::cost::PerDevice;

/// What one frame cost the engine: the time it kept each device busy, waits
/// for the display excluded. Sent to the DCC once per frame, after the frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameReport {
    /// The time the frame kept each device busy; `None` for a device the
    /// frame did not measure (never zero for "unknown").
    pub busy: PerDevice<Option<Duration>>,
}

impl FrameReport {
    /// The frame's critical path: the longest busy time among the measured
    /// devices; `None` when no device was measured.
    pub fn critical(&self) -> Option<Duration> {
        let busy = &self.busy;
        [busy.cpu, busy.gpu, busy.npu, busy.other]
            .into_iter()
            .flatten()
            .max()
    }
}
