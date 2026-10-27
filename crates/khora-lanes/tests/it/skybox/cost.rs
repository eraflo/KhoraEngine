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

//! What the skybox lane quotes on a frame's inputs: a flat time on the host,
//! recording one triangle, and a time on the device that grows with the
//! pixels the sky may cover.

use std::time::Duration;

use khora_core::lane::cost::Cost;
use khora_core::renderer::api::resource::TextureFormat;

use super::fixtures::{target, Scene, Sky};

/// The cost the run quoted for the one level.
fn quoted(sky: &mut Sky, scene: &Scene) -> Cost {
    let recorded = sky.record(scene);
    recorded.recorded();
    assert_eq!(
        recorded.run.costs.len(),
        1,
        "a run quotes the one level: {:?}",
        recorded.run.costs
    );
    recorded.run.costs[0]
}

/// **A flat host cost, a device cost per pixel.** Quadrupling the target's
/// pixels leaves the host time as it was and quadruples the device time;
/// neither is ever zero.
#[test]
fn the_sky_costs_a_flat_host_time_and_a_device_time_per_pixel() {
    let mut sky = Sky::new();
    let format = Some(TextureFormat::Depth32Float);
    let scene = sky
        .scene()
        .into_target(target(800, 600, TextureFormat::Rgba8UnormSrgb, format));
    let small = quoted(&mut sky, &scene);
    let scene = scene.into_target(target(1600, 1200, TextureFormat::Rgba8UnormSrgb, format));
    let large = quoted(&mut sky, &scene);

    assert!(small.time.cpu > Duration::ZERO, "recording is never free");
    assert_eq!(
        small.time.cpu, large.time.cpu,
        "the host records one triangle whatever the size"
    );
    assert!(small.time.gpu > Duration::ZERO, "painting is never free");
    let ratio = large.time.gpu.as_secs_f64() / small.time.gpu.as_secs_f64();
    assert!(
        (ratio - 4.0).abs() < 0.01,
        "four times the pixels, four times the device time: {:?} → {:?} (×{ratio:.3})",
        small.time.gpu,
        large.time.gpu
    );
}
