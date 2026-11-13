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
