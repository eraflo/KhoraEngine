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

//! Helpers shared by the render-side lanes: per-frame uniform buffers and bind-group layouts.

pub mod dynamic_uniform_ring_buffer;
pub(crate) mod release;
mod uniform_layout;
pub mod uniform_ring_buffer;

pub use dynamic_uniform_ring_buffer::DynamicUniformRingBuffer;
pub(crate) use uniform_layout::single_uniform_layout;
pub use uniform_ring_buffer::UniformRingBuffer;

/// The sample count of a target with `samples` samples per pixel; a count
/// the device cannot name is one sample.
pub fn sample_count(samples: u32) -> khora_core::renderer::api::resource::SampleCount {
    use khora_core::renderer::api::resource::SampleCount;
    match samples {
        2 => SampleCount::X2,
        4 => SampleCount::X4,
        8 => SampleCount::X8,
        16 => SampleCount::X16,
        32 => SampleCount::X32,
        64 => SampleCount::X64,
        _ => SampleCount::X1,
    }
}

/// Lock helper for callers that cannot propagate `Result<_, LaneError>`.
///
/// Returns the guard on success; on poisoning logs an error and
/// `return`s from the surrounding function. The optional second form
/// returns a caller-provided default value (for non-`()` returning
/// functions).
///
/// ```ignore
/// let mut res = lock_or_log!(self.gpu_resources.lock(), "MyLane::render");
/// let assets = lock_or_log!(meshes.read(), "MyLane::cost", 0.0);
/// ```
#[macro_export]
macro_rules! lock_or_log {
    ($expr:expr, $ctx:literal) => {
        match $expr {
            Ok(g) => g,
            Err(_) => {
                log::error!("{}: lock poisoned", $ctx);
                return;
            }
        }
    };
    ($expr:expr, $ctx:literal, $default:expr) => {
        match $expr {
            Ok(g) => g,
            Err(_) => {
                log::error!("{}: lock poisoned", $ctx);
                return $default;
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::sample_count;
    use khora_core::renderer::api::resource::SampleCount;

    /// Each count the device names maps to its variant; any other count is
    /// one sample.
    #[test]
    fn sample_count_names_the_device_counts() {
        for (samples, count) in [
            (1, SampleCount::X1),
            (2, SampleCount::X2),
            (4, SampleCount::X4),
            (8, SampleCount::X8),
            (16, SampleCount::X16),
            (32, SampleCount::X32),
            (64, SampleCount::X64),
        ] {
            assert_eq!(sample_count(samples), count, "{samples} samples");
        }
        for samples in [0, 3, 5, 128] {
            assert_eq!(sample_count(samples), SampleCount::X1, "{samples} samples");
        }
    }
}
