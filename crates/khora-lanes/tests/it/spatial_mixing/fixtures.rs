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

//! The sounds, sources, mix bus and frame the mixing lane's tests share.
//!
//! A frame is what the engine hands the lane: the `AudioView` the
//! `AudioFlow` projects from a world, published on the bus for that frame, an
//! `InputSource` over the bus and a runtime holding the mix bus, and an
//! `OutputSink` the lane's outputs are committed into. The View is projected
//! by the Flow rather than written by hand, so the tests hold whatever the
//! Flow puts in a snapshot.

use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use khora_core::asset::AssetHandle;
use khora_core::audio::{AudioMixBus, StreamInfo};
use khora_core::gpu::GpuSetup;
use khora_core::lane::erased::{AnyLane, Erased, Run, RunOutcome};
use khora_core::lane::io::{FrameId, InputSource, OutputSink};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::math::Vec3;
use khora_core::Runtime;
use khora_data::assets::SoundData;
use khora_data::ecs::{AudioListener, AudioSource, GlobalTransform, PlaybackState, World};
use khora_data::flow::{AudioFlow, AudioPlaybackWriteback, AudioView, Flow, Selection};
use khora_lanes::audio_lane::SpatialMixingLane;

// ─── Levels ─────────────────────────────────────────────────────────────────

/// The level mixing at most 8 voices.
pub const EIGHT_VOICES: usize = 0;
/// The level mixing at most 32 voices.
pub const THIRTY_TWO_VOICES: usize = 1;
/// The level mixing at most 128 voices.
pub const ALL_VOICES: usize = 2;
/// Each level with its voice cap, cheapest first.
pub const CAPS: [(usize, usize); 3] = [
    (EIGHT_VOICES, 8),
    (THIRTY_TWO_VOICES, 32),
    (ALL_VOICES, 128),
];

/// The stream's sample rate, and the rate of every sound that plays at it.
pub const RATE: u32 = 48_000;

// ─── Sounds ─────────────────────────────────────────────────────────────────

/// A mono sound at `rate`, from its samples.
pub fn sound(samples: Vec<f32>, rate: u32) -> AssetHandle<SoundData> {
    AssetHandle::new(SoundData {
        samples,
        channels: 1,
        sample_rate: rate,
    })
}

/// `frames` samples of `value`, at the stream's rate.
pub fn constant(value: f32, frames: usize) -> AssetHandle<SoundData> {
    sound(vec![value; frames], RATE)
}

/// `frames` samples rising from 0.001 by a millionth each, at the stream's
/// rate: where a voice is in it can be heard in the mix.
pub fn ramp(frames: usize) -> AssetHandle<SoundData> {
    sound((0..frames).map(|i| 0.001 + i as f32 * 1e-6).collect(), RATE)
}

/// `frames` samples of a sine, at `rate`.
pub fn sine(frames: usize, rate: u32) -> AssetHandle<SoundData> {
    sound((0..frames).map(|i| (i as f32).sin() * 0.1).collect(), rate)
}

/// A sound with no channel and no sample, as a placeholder asset is.
pub fn empty() -> AssetHandle<SoundData> {
    AssetHandle::new(SoundData {
        samples: Vec::new(),
        channels: 0,
        sample_rate: RATE,
    })
}

// ─── Sources ────────────────────────────────────────────────────────────────

/// One `AudioSource`, where it is and how it plays.
#[derive(Clone)]
pub struct Voice {
    pub sound: AssetHandle<SoundData>,
    pub position: Vec3,
    pub volume: f32,
    pub looping: bool,
    pub autoplay: bool,
    pub state: Option<PlaybackState>,
}

impl Voice {
    /// A source starting `sound` on its first frame, at the origin.
    pub fn playing(sound: &AssetHandle<SoundData>, volume: f32) -> Self {
        Self {
            sound: sound.clone(),
            position: Vec3::ZERO,
            volume,
            looping: false,
            autoplay: true,
            state: None,
        }
    }

    /// The same source, at `position`.
    pub fn at(mut self, position: Vec3) -> Self {
        self.position = position;
        self
    }

    /// The same source, looping.
    pub fn looping(mut self) -> Self {
        self.looping = true;
        self
    }

    /// The same source, not playing and not starting on its own.
    pub fn stopped(mut self) -> Self {
        self.autoplay = false;
        self.state = None;
        self
    }
}

/// The `AudioView` the `AudioFlow` projects from a world holding `voices`, in
/// that order, and a listener at `listener` when there is one.
pub fn view_of(listener: Option<Vec3>, voices: &[Voice]) -> AudioView {
    let mut world = World::new();
    if let Some(at) = listener {
        world.spawn((AudioListener, GlobalTransform::at_position(at)));
    }
    for voice in voices {
        let mut source = AudioSource::new(voice.sound.clone());
        source.volume = voice.volume;
        source.looping = voice.looping;
        source.autoplay = voice.autoplay;
        source.state = voice.state.clone();
        world.spawn((source, GlobalTransform::at_position(voice.position)));
    }
    let view = AudioFlow.project(&world, &Selection::new(), &Runtime::new());
    assert_eq!(
        view.sources.len(),
        voices.len(),
        "the view lists every source"
    );
    for (k, (snapshot, voice)) in view.sources.iter().zip(voices).enumerate() {
        assert!(
            snapshot.volume == voice.volume && (snapshot.position - voice.position).length() < 1e-5,
            "the view lists the sources in the order they were spawned (source {k} differs)"
        );
    }
    view
}

// ─── The mix bus ────────────────────────────────────────────────────────────

/// A mix bus keeping the last block it was given, and counting them.
///
/// The block is copied into a buffer kept across frames, so a frame after the
/// first allocates nothing here.
pub struct CapturedMix {
    info: StreamInfo,
    last: Mutex<Vec<f32>>,
    blocks: AtomicUsize,
}

impl CapturedMix {
    pub fn new(channels: u16) -> Self {
        Self {
            info: StreamInfo {
                channels,
                sample_rate: RATE,
            },
            last: Mutex::new(Vec::new()),
            blocks: AtomicUsize::new(0),
        }
    }

    /// The last block written.
    pub fn last(&self) -> Vec<f32> {
        self.last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// How many blocks were written.
    pub fn blocks(&self) -> usize {
        self.blocks.load(Ordering::SeqCst)
    }
}

impl AudioMixBus for CapturedMix {
    fn stream_info(&self) -> StreamInfo {
        self.info
    }

    fn write_block(&self, samples: &[f32]) {
        let mut last = self
            .last
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        last.clear();
        last.extend_from_slice(samples);
        self.blocks.fetch_add(1, Ordering::SeqCst);
    }

    fn pull(&self, out: &mut [f32]) {
        out.fill(0.0);
    }
}

// ─── A frame ────────────────────────────────────────────────────────────────

/// What one record gave back.
pub struct Mixed {
    /// The run as the agent sees it.
    pub run: Run,
    /// The block pushed to the mix bus during the run; empty when none was.
    pub block: Vec<f32>,
    /// How many blocks the run pushed.
    pub blocks: usize,
    /// The committed playback slot; `None` when nothing was committed.
    pub playback: Option<AudioPlaybackWriteback>,
    /// The view the run read.
    pub view: AudioView,
}

impl Mixed {
    /// Panics unless the lane recorded and its outputs were committed.
    pub fn recorded(&self) -> &AudioPlaybackWriteback {
        match &self.run.outcome {
            RunOutcome::Recorded(_) => {}
            other => panic!("the lane did not record: {other:?}"),
        }
        self.playback
            .as_ref()
            .expect("a recorded run commits the playback slot")
    }

    /// The playback state reported for the `k`-th source of the view, found
    /// by its entity.
    pub fn state_of(&self, k: usize) -> Option<PlaybackState> {
        let entity = self.view.sources[k].entity;
        let updates = &self.recorded().updates;
        let found: Vec<_> = updates.iter().filter(|u| u.entity == entity).collect();
        match found.as_slice() {
            [update] => update.new_state.clone(),
            [] => panic!("source {k} got no playback update"),
            many => panic!("source {k} got {} playback updates", many.len()),
        }
    }

    /// The cursor reported for the `k`-th source, panicking when it stopped.
    pub fn cursor_of(&self, k: usize) -> f64 {
        self.state_of(k)
            .unwrap_or_else(|| panic!("source {k} stopped, though its sound has not ended"))
            .cursor
    }
}

thread_local! {
    /// Whether this thread is inside a measured run.
    static MEASURING: Cell<bool> = const { Cell::new(false) };
}

/// Whether the calling thread is inside a measured run.
pub fn measuring() -> bool {
    MEASURING.try_with(Cell::get).unwrap_or(false)
}

/// One mixing lane, kept across frames, and the mix bus it writes to.
pub struct Mixer {
    lane: Erased<SpatialMixingLane>,
    bus: Arc<CapturedMix>,
    runtime: Runtime,
    frame: u64,
}

impl Mixer {
    /// A mixer on a stream of `channels` channels, its runtime holding the bus
    /// as the engine registers it.
    pub fn new(channels: u16) -> Self {
        let bus = Arc::new(CapturedMix::new(channels));
        let mut runtime = Runtime::new();
        runtime
            .resources
            .insert(Arc::clone(&bus) as Arc<dyn AudioMixBus>);
        Self {
            lane: Erased::new(fresh_lane()),
            bus,
            runtime,
            frame: 0,
        }
    }

    /// A mixer on a mono stream.
    pub fn mono() -> Self {
        Self::new(1)
    }

    /// A mixer on a stereo stream.
    pub fn stereo() -> Self {
        Self::new(2)
    }

    /// A mixer whose runtime holds no mix bus.
    pub fn without_mix_bus() -> Self {
        let mut mixer = Self::stereo();
        mixer.runtime = Runtime::new();
        mixer
    }

    /// Records the next frame at `level` over `view`.
    pub fn mix(&mut self, level: usize, view: AudioView) -> Mixed {
        self.run(level, Some(view), false)
    }

    /// Records the next frame at `level` with no View on the bus.
    pub fn mix_without_view(&mut self, level: usize) -> Mixed {
        self.run(level, None, false)
    }

    /// Records the next frame at `level` over `view`, with
    /// [`measuring`] true on this thread for the run alone.
    pub fn mix_measured(&mut self, level: usize, view: AudioView) -> Mixed {
        self.run(level, Some(view), true)
    }

    fn run(&mut self, level: usize, view: Option<AudioView>, measured: bool) -> Mixed {
        self.frame += 1;
        let frame = FrameId(self.frame);
        let mut bus = LaneBus::new();
        bus.set_frame(frame);
        let read = view.clone().unwrap_or_default();
        if let Some(view) = view {
            bus.publish(view);
        }
        let blocks_before = self.bus.blocks();
        let mut deck = OutputDeck::new();
        let run = {
            let mut inputs = InputSource::new(frame, &bus, &self.runtime);
            let mut outputs = OutputSink::new(&mut deck);
            let mut gpu = GpuSetup::headless();
            if measured {
                MEASURING.with(|m| m.set(true));
            }
            let run = self.lane.run(level, &mut inputs, &mut outputs, &mut gpu);
            if measured {
                MEASURING.with(|m| m.set(false));
            }
            run
        };
        let blocks = self.bus.blocks() - blocks_before;
        Mixed {
            run,
            block: if blocks > 0 {
                self.bus.last()
            } else {
                Vec::new()
            },
            blocks,
            playback: committed(&deck),
            view: read,
        }
    }
}

/// The playback slot `deck` holds, if the run committed one.
fn committed(deck: &OutputDeck) -> Option<AudioPlaybackWriteback> {
    deck.get::<AudioPlaybackWriteback>().cloned()
}

// ─── Comparing mixes ────────────────────────────────────────────────────────

/// The largest difference between two blocks of the same length.
pub fn max_difference(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "the blocks have the same length");
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// Panics unless `actual` sounds like `expected`, sample for sample.
pub fn assert_same_mix(actual: &[f32], expected: &[f32], what: &str) {
    assert!(!expected.is_empty(), "the reference mix wrote a block");
    let difference = max_difference(actual, expected);
    assert!(
        difference < 1e-6,
        "{what}: the mixes differ by up to {difference}"
    );
}

/// The sum of the squares of every `step`-th sample from `first`.
pub fn energy(block: &[f32], first: usize, step: usize) -> f32 {
    block.iter().skip(first).step_by(step).map(|s| s * s).sum()
}

/// A lane as its registration builds it: by `Default`.
fn fresh_lane() -> SpatialMixingLane {
    Default::default()
}
