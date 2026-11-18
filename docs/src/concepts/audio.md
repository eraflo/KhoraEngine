# Audio

<p class="kp-lede">A sound in a game comes from somewhere: the footsteps behind you, the fire on your left. Khora places every sound in the world and mixes what one pair of ears would hear there. When more sounds play than the machine can mix in its time, the quietest are kept silent for a moment, still moving through their sound, so that each one comes back where it would have been.</p>

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
the lane writes mixed samples into the bus, the callback reads from it on the audio
thread, and neither waits for the other. As everywhere else in the engine, the
agent and lane call the trait, never the backend directly.

The bus holds a bounded amount of sound. When the lane writes more than it holds,
the **oldest** samples go: in real time a stale sample is worse than silence. When
the callback asks for more than the bus holds, it gets what there is and silence
for the rest. The default bus (`DefaultMixBus`, in `khora-infra`) holds 8192
frames of stereo at 48 kHz, about 170 ms.

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
  ↓ the most audible sources mixed: distance attenuation + stereo panning
audio mix bus  ──→  device callback fills the output buffer
```

The lane never queries the World and never touches the device directly. It reads
the projected View, mixes, and writes the bus — a clean CLAD descent. Where each
source is in its sound (its playback cursor) is written into the family's deck,
and a maintenance step applies it back onto the source components, so the lane
itself stays a read-and-write-outputs projector rather than a World mutator.

The audio family honours a **scope**: switched on for the entities holding one
marker component only (`Activation::scope`, as the editor's sound preview does),
it mixes those entities' sources and no other.

## The spatial model

Two ECS components define the scene. A **source** carries the sound handle, a
volume, a priority and looping/autoplay flags; a **listener** marks the entity
whose pose is the ear. The first entity carrying the listener component (together
with a world-space transform) is the active listener.

For each source, the mixing lane computes the vector from the listener to the
source. Its length drives **distance attenuation**: the gain falls with the
inverse square of the distance, `volume / (1 + d²)`, so a source is never cut off
at a distance, only quieter. Its lateral component drives **stereo pan**. A scene
with no listener mixes every source at its own volume, centred. This is the whole
spatial model — distance and direction relative to one listener — and it is
intentionally simple. Effects like reverb, HRTF, and filters are real and valuable
but not yet implemented; the mix bus is the seam where they will plug in.

Source playback is tied to entity lifetime: there is no global "playing sounds"
registry to manage. The concrete component API for spawning a source and a listener
lives in [How-to: play 3D audio](../how-to/play-3d-audio.md).

## Voices: the most audible are mixed

Mixing a source costs the host time on every frame. So the lane mixes at most a
fixed number of **voices**, and its levels are those numbers: **8**, **32** or
**128** voices. GORNA lends the audio family an envelope and its agent picks the
level that fits.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/audio/voices.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>More sounds than voices.</strong> Five sounds play; this level mixes three. The engine's levels mix 8, 32 or 128.</li>
<li><strong>Ranked.</strong> The highest priority first, then the loudest at the listener. The first three are mixed; the others are <em>virtual</em> — silent, their place in the sound still moving.</li>
<li><strong>The listener walks.</strong> The bell is now nearer than the steps: it takes a voice, and the steps become virtual.</li>
<li><strong>Back in place.</strong> The bell is heard from where it would have been, not from its start, because its place kept moving while it was silent.</li>
</ol>
</figure>

A source's **priority** (`AudioSource::priority`, 0–255, 128 by default) wins over
loudness: a higher priority keeps its voice however loud the others are; equal
priorities are ranked by their gain at the listener. A **virtual voice** costs
almost nothing (its cursor advances by the block's length) and is never heard.

The family's semantics are `Presentation`: a cheaper level mixes fewer voices,
never a different playback. A sound that is virtual for a while is not delayed
nor restarted; it is only not heard. The 8- and 32-voice levels say so in their
quote (`Impact::Minor`); the 128-voice level is the reference.

## The default backend — CPAL

The default `AudioDevice` is a CPAL wrapper in `khora-infra/src/audio/cpal/`. CPAL
provides cross-platform device enumeration, format negotiation, and the callback
loop; Khora wraps it in the `AudioDevice` contract and delivers the mixed buffer
through its callback. A different backend — a platform-native API, or web audio in a
future browser target — drops in as a new implementation of the same trait, and the
lane never notices. That swappability is, again, the reason the device is a trait
rather than a direct call.

## Under the hood

**One block a frame.** Each run mixes one block of up to 1024 frames — a display
frame at 60 Hz and 48 kHz is about 800 — into the bus.

**Ranking.** For a source *s* with volume *v*, priority *p* and distance *d* to
the listener, the gain is *g(s) = v / (1 + d²)*. Sources are ordered by *p*
descending, then *g* descending, and the first *n* are mixed, *n* being the
level's cap. Every other playing source is virtual: its cursor advances by
*frames × ratio*, the same amount a mixed voice's would, *ratio* being its sample
rate over the stream's. A source's cursor is an `f64`, so a long track does not
drift off its rate as an `f32` would after a few minutes.

**Cost.** The lane prices a run as a fixed block cost plus about 4 µs per mixed
voice and 50 ns per virtual one, all on the host.

**The bus's lock** is a leaf, held only to push or pull samples, so the audio
thread and the lane never wait on each other for longer than one copy.

## Next steps

- [How-to: play 3D audio](../how-to/play-3d-audio.md) — spawn a source and a
  listener with the real component API.
- [Data and the ECS](./ecs.md) — how audio components are stored and projected.
- [SAA](./saa.md) — how the audio agent negotiates its voices under pressure.
- [Glossary](../reference/glossary.md) — AudioDevice, mix bus, Flow, lane.
