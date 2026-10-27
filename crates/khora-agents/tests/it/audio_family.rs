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

//! The audio family, registered and booted as the engine boots it: one
//! `LaneAgent` built from the registration the family submitted, checked
//! against the runtime before the first frame.
//!
//! These replace the audio agent's own tests: what it declared, that it was
//! world-free, and the lanes it held.

use std::any::TypeId;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use khora_agents::lane_agent::lane_agent;
use khora_agents::registered_agent::{lane_agents, registered_agents};
use khora_core::agent::gorna::AgentId;
use khora_core::agent::registration::{check_registrations, AgentRegistration, BootError};
use khora_core::agent::{Agent, AgentAccess, AgentImportance, ExecutionPhase};
use khora_core::asset::AssetHandle;
use khora_core::audio::{AudioMixBus, StreamInfo};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::family::Semantics;
use khora_core::lane::guard::{SinkKind, SourceKind};
use khora_core::lane::io::FrameId;
use khora_core::lane::{LaneBus, LaneKind, OutputDeck};
use khora_core::math::Vec3;
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::assets::SoundData;
use khora_data::ecs::{AudioSource, GlobalTransform, World};
use khora_data::flow::{AudioFlow, AudioPlaybackWriteback, AudioView, Flow, Selection};
use khora_lanes::audio_lane::AudioFamily;

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// The audio family's registration, as the engine collects it.
pub fn audio_registration() -> &'static AgentRegistration {
    let found: Vec<&'static AgentRegistration> = registered_agents()
        .into_iter()
        .filter(|registration| registration.family == TypeId::of::<AudioFamily>())
        .collect();
    match found.as_slice() {
        [one] => one,
        [] => panic!(
            "the audio family is not registered: no registration of `AudioFamily` was submitted"
        ),
        many => panic!("the audio family is registered {} times", many.len()),
    }
}

/// A mix bus counting the blocks it is given.
struct CountingMix {
    blocks: AtomicUsize,
}

impl AudioMixBus for CountingMix {
    fn stream_info(&self) -> StreamInfo {
        StreamInfo {
            channels: 2,
            sample_rate: 48_000,
        }
    }

    fn write_block(&self, _samples: &[f32]) {
        self.blocks.fetch_add(1, Ordering::SeqCst);
    }

    fn pull(&self, out: &mut [f32]) {
        out.fill(0.0);
    }
}

/// A runtime holding `mix` as the engine registers the mix bus.
fn runtime_with(mix: &Arc<CountingMix>) -> Runtime {
    let mut runtime = Runtime::new();
    runtime
        .resources
        .insert(Arc::clone(mix) as Arc<dyn AudioMixBus>);
    runtime
}

fn counting_mix() -> Arc<CountingMix> {
    Arc::new(CountingMix {
        blocks: AtomicUsize::new(0),
    })
}

/// The view the `AudioFlow` projects from `count` playing sources.
fn view_of_sources(count: usize) -> AudioView {
    let tone = AssetHandle::new(SoundData {
        samples: vec![0.01; 4096],
        channels: 1,
        sample_rate: 48_000,
    });
    let mut world = World::new();
    for k in 0..count {
        world.spawn((
            AudioSource::new(tone.clone()),
            GlobalTransform::at_position(Vec3::new(k as f32, 0.0, 0.0)),
        ));
    }
    AudioFlow.project(&world, &Selection::new(), &Runtime::new())
}

// ─── The registration ───────────────────────────────────────────────────────

/// **The audio family registers by submitting**, once, as the agent it
/// replaces ran: an `Audio` agent in `TRANSFORM`, important, in every mode,
/// every frame, with one lane, the spatial mixer.
#[test]
fn the_audio_family_is_registered_once() {
    let registration = audio_registration();

    assert_eq!(registration.family_name, "audio");
    assert_eq!(registration.agent, AgentId::Audio);
    assert_eq!(registration.kind, LaneKind::Audio);
    assert_eq!(registration.semantics, Semantics::Presentation);
    assert_eq!(registration.phase, ExecutionPhase::TRANSFORM);
    assert_eq!(registration.importance, AgentImportance::Important);
    assert!(registration.modes.is_empty(), "audio runs in every mode");
    assert_eq!(registration.fixed_step, None, "audio runs every frame");
    let lanes: Vec<&'static str> = registration
        .lanes
        .iter()
        .map(|make| make().name())
        .collect();
    assert_eq!(lanes.len(), 1, "one lane: {lanes:?}");
    assert!(
        lanes[0].ends_with("SpatialMixingLane"),
        "the spatial mixer: {lanes:?}"
    );
}

/// The lane declares what it reads — the `AudioView` from the bus and the mix
/// bus from the runtime — and that it locks nothing; its family declares the
/// playback slot it writes.
#[test]
fn the_audio_lane_declares_the_view_and_the_mix_bus_it_reads() {
    let lane = (audio_registration().lanes[0])();
    let declaration = lane.declaration();

    let reads = |kind: SourceKind, ty: TypeId| {
        declaration
            .inputs()
            .iter()
            .any(|input| input.kind == kind && input.ty == ty)
    };
    assert!(
        reads(SourceKind::Bus, TypeId::of::<AudioView>()),
        "the AudioView from the bus: {:?}",
        declaration.inputs()
    );
    assert!(
        reads(SourceKind::Resource, TypeId::of::<Arc<dyn AudioMixBus>>()),
        "the mix bus from the runtime, read without a lock: {:?}",
        declaration.inputs()
    );
    assert!(
        declaration
            .inputs()
            .iter()
            .all(|input| input.kind != SourceKind::Locked),
        "the mix bus is shared through `&self`: nothing is locked, {:?}",
        declaration.inputs()
    );
    assert!(
        declaration
            .outputs()
            .iter()
            .any(|output| output.kind == SinkKind::Deck
                && output.ty == TypeId::of::<AudioPlaybackWriteback>()),
        "the playback slot: {:?}",
        declaration.outputs()
    );
}

// ─── Boot ───────────────────────────────────────────────────────────────────

/// **The audio family boots** on a runtime holding the mix bus: the check
/// accepts it, and one `Audio` agent is built from it.
#[test]
fn the_audio_family_boots_with_a_mix_bus() {
    let runtime = runtime_with(&counting_mix());
    let registration = audio_registration();

    let checked = check_registrations(&[registration], &runtime, &DeviceCaps::default());
    assert_eq!(checked, Ok(()), "the check accepts the audio family");

    let agents = match lane_agents(&[registration], &runtime, &DeviceCaps::default()) {
        Ok(agents) => agents,
        Err(refusal) => panic!("the audio family boots: {refusal}"),
    };
    let ids: Vec<AgentId> = agents.iter().map(|agent| agent.id()).collect();
    assert_eq!(ids, vec![AgentId::Audio]);
}

/// Boot refuses the audio family on a runtime without the mix bus, naming the
/// family and the resource: the lane could never run.
#[test]
fn boot_refuses_the_audio_family_without_a_mix_bus() {
    let checked = check_registrations(
        &[audio_registration()],
        &Runtime::new(),
        &DeviceCaps::default(),
    );

    match checked {
        Err(BootError::UnknownResource {
            family, resource, ..
        }) => {
            assert_eq!(family, "audio");
            assert!(
                resource.contains("AudioMixBus"),
                "the missing resource is the mix bus: {resource}"
            );
        }
        other => panic!("expected the mix bus to be missing, got {other:?}"),
    }
}

// ─── The agent ──────────────────────────────────────────────────────────────

/// The audio agent's contention is its lane's: it writes the playback slot, and
/// reads the mix bus without locking it.
#[test]
fn the_audio_agent_writes_its_playback_slot_and_reads_the_mix_bus() {
    let agent = lane_agent(audio_registration(), &DeviceCaps::default());
    let contention = agent.contention();
    let mix_bus = TypeId::of::<Arc<dyn AudioMixBus>>();

    assert!(
        contention
            .deck
            .contains(&TypeId::of::<AudioPlaybackWriteback>()),
        "the playback slot is declared: {:?}",
        contention.deck
    );
    assert!(contention.reads.contains(&mix_bus), "the mix bus is read");
    assert!(
        !contention.writes.contains(&mix_bus),
        "the mix bus is not locked: a lock would serialise the wave"
    );
}

/// The audio agent never reaches the `World`: it reads a View and mixes into
/// the bus, so it may run on the worker pool.
#[test]
fn the_audio_agent_is_world_free() {
    let agent = lane_agent(audio_registration(), &DeviceCaps::default());

    assert_eq!(agent.access(), AgentAccess::Isolated);
}

/// **A frame through the agent**, as the scheduler runs it: the agent mixes
/// the frame's sources into one block on the mix bus and commits one playback
/// update per source into the deck.
#[test]
fn the_audio_agent_mixes_a_frame_into_its_playback_slot() {
    let mix = counting_mix();
    let runtime = Arc::new(runtime_with(&mix));
    let mut agent = lane_agent(audio_registration(), &DeviceCaps::default());
    let mut bus = LaneBus::new();
    bus.set_frame(FrameId(1));
    bus.publish(view_of_sources(3));
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    let id = agent.id();

    {
        let mut context = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(&runtime),
            &bus,
            &mut deck,
            &permit,
            Some(id),
        );
        agent.execute(&mut context);
    }

    let playback = deck
        .get::<AudioPlaybackWriteback>()
        .expect("the agent committed the playback slot");
    assert_eq!(playback.updates.len(), 3, "one update per source");
    assert!(
        playback
            .updates
            .iter()
            .all(|update| update.new_state.is_some()),
        "every source is playing"
    );
    assert_eq!(mix.blocks.load(Ordering::SeqCst), 1, "one block was mixed");
}
