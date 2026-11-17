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

//! What a level costs one frame, per device and per link.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::lane::level::Impact;

pub use super::device::Device;

/// One value per kind of device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerDevice<T> {
    /// The host processor.
    pub cpu: T,
    /// The graphics processor.
    pub gpu: T,
    /// The neural processor.
    pub npu: T,
    /// Any other device a backend brings.
    pub other: T,
}

/// One value per link between devices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerLink<T> {
    /// Host to graphics processor.
    pub cpu_to_gpu: T,
    /// Graphics processor to host.
    pub gpu_to_cpu: T,
}

/// How far a cost can be trusted, from `0.0` (a guess) to `1.0` (measured).
#[derive(Debug, Clone, Copy, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(pub f32);

/// What running one level costs this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// The time spent on each device.
    pub time: PerDevice<Duration>,
    /// The bytes sent on each link.
    pub transfer: PerLink<u64>,
    /// The bytes held on each device.
    pub memory: PerDevice<u64>,
    /// How far the figures can be trusted.
    pub confidence: Confidence,
    /// How much running the level on these inputs changes the result; `None`
    /// means the level's own [`Level::impact`](crate::lane::level::Level::impact).
    #[serde(default)]
    pub impact: Option<Impact>,
    /// The share of the frame's due work the level serves, from `0.0` to `1.0`.
    ///
    /// `None` means all of it. A row's quality is its impact's quality times
    /// this share.
    #[serde(default)]
    pub share: Option<f32>,
}

impl Cost {
    /// Returns the share of the due work served, from `0.0` to `1.0`.
    ///
    /// No `share` or one above one is all of it; one below zero or NaN is none.
    pub fn served(&self) -> f32 {
        match self.share {
            None => 1.0,
            Some(share) if share.is_nan() => 0.0,
            Some(share) => share.clamp(0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A cost saved before quotes named a share reads back serving all the
    /// work**: `share` is `None` when the field is absent.
    #[test]
    fn a_cost_without_a_share_field_reads_back_with_no_share() {
        let mut saved = serde_json::to_value(Cost::default()).expect("a cost serialises");
        saved
            .as_object_mut()
            .expect("a cost is an object")
            .remove("share");

        let read: Cost = serde_json::from_value(saved).expect("a cost without a share reads");

        assert_eq!(read.share, None);
    }

    /// **The share served is held within `0.0..=1.0`**: no share is all of
    /// the work, one that is not a number is none of it.
    #[test]
    fn the_share_served_is_held_within_zero_and_one() {
        let served = |share| {
            Cost {
                share,
                ..Cost::default()
            }
            .served()
        };

        assert_eq!(served(None), 1.0);
        assert_eq!(served(Some(0.25)), 0.25);
        assert_eq!(served(Some(2.0)), 1.0);
        assert_eq!(served(Some(-1.0)), 0.0);
        assert_eq!(served(Some(f32::NAN)), 0.0);
    }
}
