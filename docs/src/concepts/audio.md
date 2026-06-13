# Audio

3D positional audio behind a single device trait, mixed on a spatial lane. This
page explains *why* audio is a swappable device fed through a mix bus, and how the
ECS audio domain reaches the speakers. It is an explanation, not a recipe — for the
component usage that actually plays a sound, see
[How-to: play 3D audio](../how-to/play-3d-audio.md).

---

## The contract is one trait, plus a bus

The audio surface is the **`AudioDevice`** trait in `khora-core`
(`audio/device.rs`). It is deliberately narrow: a device is a *factory* for an
output stream. You hand it the **audio mix bus** and it opens a long-lived stream
whose hardware callback pulls mixed samples from that bus on a dedicated real-time
thread; dropping the stream stops it. The device has no per-frame API to call — the
bus is the sole synchronisation boundary between the engine's frame loop and the
audio thread.

This is the shape worth understanding. The two worlds run at different rates: the
engine mixes at frame rate, the hardware callback drains at the device's sample
rate (commonly 48 kHz). Rather than couple them, the **mix bus** decouples them —
the lane writes mixed samples into the bus from the main thread, the callback reads
from it on the audio thread, and neither blocks the other. As everywhere else in
the engine, the agent and lane call the trait, never the backend directly.

## How sound reaches the speakers

Audio is one slice of the per-frame descent (`Control → Agent → Lane → Data`). The
data layer's audio **[Flow](../reference/glossary.md)** projects the ECS audio
domain — the active listener's pose and a snapshot of every sound source — into a
View on the LaneBus. The **spatial mixing lane** consumes that View, does the
spatial math, and writes the result into the mix bus the device's callback reads:

```
ECS (sources, listener, transforms)
  ↓ audio Flow projects a View into the LaneBus
spatial mixing lane
  ↓ distance attenuation + stereo panning
audio mix bus  ──→  device callback fills the output buffer
```

The lane never queries the World and never touches the device directly. It reads
the projected View, mixes, and writes the bus — a clean CLAD descent. A companion
maintenance step applies any playback-state changes the lane produced back onto the
source components, so the lane itself stays a read-and-write-outputs projector
rather than a World mutator.

## The spatial model

Two ECS components define the scene. A **source** carries the sound handle, a
volume, and looping/autoplay flags; a **listener** marks the entity whose pose is
the ear. The first entity carrying the listener component (together with a
world-space transform) is the active listener.

For each source, the mixing lane computes the vector from the listener to the
source: its length drives **distance attenuation**, and its lateral component
drives **stereo pan**. A source far enough away contributes nothing and is skipped;
a source with no listener present is mixed without spatialisation. This is the whole
spatial model — distance and direction relative to one listener — and it is
intentionally simple. Effects like reverb, HRTF, and filters are real and valuable
but not yet implemented; the mix bus is the seam where they will plug in.

Source playback is tied to entity lifetime: there is no global "playing sounds"
registry to manage. The concrete component API for spawning a source and a listener
lives in [How-to: play 3D audio](../how-to/play-3d-audio.md).

## The default backend — CPAL

The default `AudioDevice` is a CPAL wrapper in `khora-infra/src/audio/cpal/`. CPAL
provides cross-platform device enumeration, format negotiation, and the callback
loop; Khora wraps it in the `AudioDevice` contract and delivers the mixed buffer
through its callback. A different backend — a platform-native API, or web audio in a
future browser target — drops in as a new implementation of the same trait, and the
lane never notices. That swappability is, again, the reason the device is a trait
rather than a direct call.

## Next steps

- [How-to: play 3D audio](../how-to/play-3d-audio.md) — spawn a source and a
  listener with the real component API.
- [Data and the ECS](./ecs.md) — how audio components are stored and projected.
- [SAA](./saa.md) — how the audio agent negotiates a source budget under pressure.
- [Glossary](../reference/glossary.md) — AudioDevice, mix bus, Flow, lane.
