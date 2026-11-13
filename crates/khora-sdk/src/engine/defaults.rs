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

//! The backends an application may bring, and the engine's own when it
//! brings none: the audio mix bus, the text shaper and glyph rasteriser, the
//! physics provider and its settings.

use khora_core::Runtime;
use std::sync::{Arc, Mutex};

/// Installs in `runtime` the engine's own audio bus, text shaper, glyph
/// rasteriser, physics provider and physics settings, each only where the
/// application installed none.
pub(super) fn install_defaults(runtime: &mut Runtime) {
    // The audio family mixes into the shared bus every frame. An application
    // that opens an audio device installs the bus it hands the device; without
    // one, the engine keeps a bus of its own, which nothing drains.
    if !runtime
        .resources
        .contains::<Arc<dyn khora_core::audio::AudioMixBus>>()
    {
        let mix_bus: Arc<dyn khora_core::audio::AudioMixBus> =
            Arc::new(khora_infra::DefaultMixBus::new(
                khora_core::audio::StreamInfo {
                    channels: 2,
                    sample_rate: 48_000,
                },
                8192,
            ));
        runtime.resources.insert(mix_bus);
    }

    // Texts are shaped by `ui_text_layout` through the shaper, and their
    // glyphs rasterised by `glyphs_on_gpu` through the rasteriser. An
    // application that brings either keeps its own; the engine's are Parley
    // and swash, which share the built-in font.
    if !runtime
        .backends
        .contains::<Arc<dyn khora_core::ui::text::TextShaper>>()
    {
        let shaper: Arc<dyn khora_core::ui::text::TextShaper> =
            Arc::new(khora_infra::ParleyShaper::new());
        runtime.backends.insert(shaper);
    }
    if !runtime
        .backends
        .contains::<Arc<dyn khora_core::ui::text::GlyphRasterizer>>()
    {
        let rasterizer: Arc<dyn khora_core::ui::text::GlyphRasterizer> =
            Arc::new(khora_infra::SwashRasterizer::new());
        runtime.backends.insert(rasterizer);
    }

    // The physics family steps the provider every fixed sub-step, at the
    // game's `physics.step`. An application that brings neither gets the
    // engine's own: Rapier, at the default step.
    if runtime
        .look_up::<Arc<Mutex<Box<dyn khora_core::physics::PhysicsProvider>>>>()
        .is_none()
    {
        let physics: Box<dyn khora_core::physics::PhysicsProvider> =
            Box::new(khora_infra::physics::rapier::RapierPhysicsWorld::default());
        runtime.backends.insert(Arc::new(Mutex::new(physics)));
    }
    if !runtime
        .resources
        .contains::<khora_core::physics::PhysicsSettings>()
    {
        runtime
            .resources
            .insert(khora_core::physics::PhysicsSettings::default());
    }
}
