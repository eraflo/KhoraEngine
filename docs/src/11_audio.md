# Audio

3D positional audio through the `AudioDevice` trait. Default backend is CPAL.

- Document — Khora Audio v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. The contract
2. Pipeline
3. Components
4. Spatial mixing
5. The default backend — CPAL
6. AudioAgent and GORNA
7. For game developers
8. For engine contributors
9. Decisions
10. Open questions

---

## 01 — The contract

The audio surface is a single trait in `khora-core`:

```rust
pub trait AudioDevice: Send + Sync {
    fn start(&mut self, callback: Box<dyn FnMut(&mut [f32]) + Send>);
    fn stop(&mut self);
    fn sample_rate(&self) -> u32;
    fn channels(&self) -> u16;
}
```

`AudioAgent` does not call CPAL. It calls `AudioDevice`. The current implementation is `CpalAudioDevice` in `khora-infra`. A different audio backend (XAudio2, OpenAL, web audio in a future browser target) drops in as a new `khora-infra/src/audio/<backend>/`.

## 02 — Pipeline

```
ECS (AudioSource, AudioListener, GlobalTransform)
  ↓
SpatialMixingLane::execute()
  ↓ distance attenuation, directional mixing
AudioDevice::start() → callback fills output buffer
```

Each frame, `SpatialMixingLane`:

1. Finds the active `AudioListener` in the ECS.
2. For every `AudioSource`, computes distance and direction relative to the listener.
3. Applies attenuation curves and panning.
4. Mixes the result into the output buffer the audio callback consumes.

The mix runs at frame rate; the callback runs at the device's sample rate (typically 48 kHz). The two are decoupled through a ring buffer.

## 03 — Components

| Component | Purpose |
|---|---|
| `AudioSource` | Audio clip handle, volume, spatial flag, looping flag |
| `AudioListener` | Marks the entity whose position is the listener's position |
| `GlobalTransform` | World-space pose — provides the source / listener position |

The active listener is the entity that has both `AudioListener` and `GlobalTransform`. If multiple exist, the first registered is used (this may become a configurable selection in a future version).

## 04 — Spatial mixing

The `SpatialMixingLane` does the mathematical work:

| Step | Computation |
|---|---|
| **Distance** | `||source.position - listener.position||` |
| **Attenuation** | Inverse-square with floor and ceiling parameters |
| **Direction** | Vector from listener to source, transformed into listener space |
| **Pan** | Direction's lateral component → stereo balance |

Sources beyond a distance threshold are culled (no mix work). Sources without spatial flag set are mixed without 3D processing — they are 2D sources (UI sounds, music).

Audio formats supported through Symphonia: WAV, Ogg Vorbis, MP3, FLAC. The decoder is a separate lane (`SymphoniaLoaderLane` or `WavLoaderLane`) — see [Assets and VFS](./12_assets.md).

## 05 — The default backend — CPAL

| File | Purpose |
|---|---|
| `crates/khora-infra/src/audio/cpal/` | `CpalAudioDevice` — implements `AudioDevice` |

CPAL provides the cross-platform device enumeration, format negotiation, and callback loop. Khora wraps it in the `AudioDevice` contract and delivers the mixed buffer through the callback.

To swap to another backend: implement `AudioDevice` in `khora-infra/src/audio/<backend>/`, register it as a service. Lanes never see the change — they hold the trait object.

## 06 — AudioAgent and GORNA

`AudioAgent` exposes three strategies based on the source budget:

| Strategy | Max sources | When |
|---|---|---|
| **Full** | 64 | Healthy budget |
| **Reduced** | 16 | Mid-pressure |
| **Minimal** | 4 | Heavy pressure or low battery |

Beyond the budget, sources are culled by priority — distance from listener and clip volume. The clip itself is not stopped at the source level; the mixer skips it for that frame.

---

## For game developers

```rust
// The listener (usually attached to the camera)
world.spawn((
    Transform::default(),
    GlobalTransform::identity(),
    Camera::default(),
    AudioListener::default(),
));

// A 3D positional sound
let clip = asset_service.load::<SoundData>("sfx/footstep.wav").await?;
world.spawn((
    Transform::from_translation(footstep_pos),
    GlobalTransform::identity(),
    AudioSource::spatial(clip).with_volume(0.7),
));

// A 2D sound (no spatial processing)
let music = asset_service.load::<SoundData>("music/theme.ogg").await?;
world.spawn((
    AudioSource::ambient(music).with_loop(true),
));
```

To stop a source, despawn the entity. To pause, mute the volume to zero. Audio playback is tied to entity lifetime — there is no global "playing sounds" registry to manage.

## For engine contributors

The split mirrors physics:

| File | Purpose |
|---|---|
| `crates/khora-core/src/audio/` | `AudioDevice` trait, audio types |
| `crates/khora-lanes/src/audio_lane/spatial_mixing.rs` | `SpatialMixingLane` — distance, direction, panning |
| `crates/khora-agents/src/audio_agent/mod.rs` | `AudioAgent` — source budget, GORNA negotiation |
| `crates/khora-infra/src/audio/cpal/` | CPAL backend |

To add a new mixing strategy (HRTF, ambisonics): create a new lane under `audio_lane/`, expose it from `AudioAgent::negotiate` with cost estimate. The current `SpatialMixingLane` stays as the default.

## Decisions

### We said yes to
- **A single trait surface.** `AudioDevice` is the only seam between Khora and the audio platform.
- **Source budget as the primary GORNA dimension.** Audio scales linearly with source count; the budget is a count.
- **Listener-tied to ECS.** The listener follows whatever entity has the component, no global state.
- **2D and 3D sources distinguished by flag.** No separate APIs.

### We said no to
- **Calling CPAL directly from anywhere except the backend folder.** Same rule as everywhere else.
- **A "global music" channel.** Music is just an `AudioSource` without the spatial flag. Less special-casing.
- **DSP effects in v1.** Reverb, EQ, filters — all real, all valuable, not yet implemented. The ring buffer is the seam where they will plug in.

## Open questions

1. **HRTF (head-related transfer function) for headphones.** Better spatialization for headphone users. Library candidates exist; integration is not designed.
2. **Listener selection.** Today, first-registered wins. Multiple listeners (split-screen, recording) need an explicit selection model.
3. **Convolution reverb.** Real-time convolution is feasible on modern hardware; the API for impulse responses is undecided.

---

*Next: how assets get loaded. See [Assets and VFS](./12_assets.md).*
