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

//! `AudioFlow` — initial stub for the audio domain Flow.
//!
//! Currently publishes a small statistics view. A full migration of the
//! spatial-mixing lane to consume from the [`LaneBus`] (with AGDF-style
//! `adapt` to detach far-away `AudioSource`s) is tracked separately.

use khora_core::math::Vec3;
use khora_core::ServiceRegistry;

use crate::ecs::{AudioListener, AudioSource, GlobalTransform, SemanticDomain, World};
use crate::flow::{Flow, Selection};
use crate::register_flow;

/// View published by [`AudioFlow`]. Carries per-frame audio domain stats.
#[derive(Debug, Default, Clone)]
pub struct AudioView {
    /// Number of `AudioSource` components in the world.
    pub source_count: usize,
    /// World-space position of the first `AudioListener`, if any.
    pub listener_position: Option<Vec3>,
}

/// Stub Flow that surfaces the audio domain in the registry.
#[derive(Default)]
pub struct AudioFlow;

impl Flow for AudioFlow {
    type View = AudioView;

    const DOMAIN: SemanticDomain = SemanticDomain::Audio;
    const NAME: &'static str = "audio";

    fn project(&self, world: &World, _sel: &Selection, _services: &ServiceRegistry) -> Self::View {
        let source_count = world.query::<&AudioSource>().count();
        let listener_position = world
            .query::<(&AudioListener, &GlobalTransform)>()
            .next()
            .map(|(_, t)| t.0.translation());
        AudioView {
            source_count,
            listener_position,
        }
    }
}

register_flow!(AudioFlow);
