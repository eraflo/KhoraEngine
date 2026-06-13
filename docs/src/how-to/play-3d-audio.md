# Play 3D positional audio

This guide shows you how to put a listener on the camera, emit a positional sound from an
entity, and control its volume and looping.

> **Prerequisites.** Your app wires the audio backend in the `run_winit` bootstrap
> (the CPAL device + mix bus, as the sandbox does) so the mixer has somewhere to send
> samples. You have a `SoundData` handle ([Load and reference assets](./load-assets.md)).

## Mark the listener

The listener is the entity whose `GlobalTransform` defines where the ears are — usually
the camera. Add an `AudioListener` (a marker component) alongside the camera's transform.
There should be exactly one; if several exist, the first encountered wins.

```rust
use khora_sdk::khora_data::ecs::AudioListener;
use khora_sdk::prelude::ecs::Camera;
use khora_sdk::prelude::math::Vec3;

let camera = Camera::new_perspective(std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 1000.0);
khora_sdk::Vessel::at(world, Vec3::new(0.0, 2.0, 10.0))
    .with_component(camera)
    .with_component(AudioListener)
    .build();
```

## Emit a positional sound

An `AudioSource` carries an `AssetHandle<SoundData>` plus playback flags. Build one with
`AudioSource::new(handle)` (which sets `autoplay = true`) and place it in the world with a
`Transform` + `GlobalTransform`. Spatialization is automatic: a source that has a
`GlobalTransform` is panned and attenuated relative to the listener.

```rust
use khora_sdk::khora_data::ecs::AudioSource;
use khora_sdk::prelude::ecs::{GlobalTransform, Transform};
use khora_sdk::prelude::math::Vec3;

// `clip: AssetHandle<SoundData>` from the asset service or a decoded buffer.
let mut footstep = AudioSource::new(clip);
footstep.volume = 0.7;

world.spawn((
    Transform::from_translation(Vec3::new(3.0, 0.0, -6.0)),
    GlobalTransform::default(),
    footstep,
));
```

## Control volume and looping

`AudioSource` fields are plain data — set them at spawn time, or mutate them later through
a query:

```rust
use khora_sdk::khora_data::ecs::AudioSource;

let mut music = AudioSource::new(theme);
music.looping = true;   // restart on end
music.volume = 0.5;     // linear gain; 1.0 is unattenuated
music.autoplay = true;  // begin on first encounter
```

To change a live source, query it mutably in `update`:

```rust
for (source,) in world.query_mut::<(&mut AudioSource,)>() {
    source.volume = 0.2;
}
```

A source with **no** `GlobalTransform` is mixed as a 2D sound (UI clicks, music) — no
distance attenuation or panning. To stop a sound, despawn its entity; to silence it, set
`volume` to `0.0`. Playback is tied to entity lifetime — there is no global "playing
sounds" registry.

## What the spatial mixer does

Each frame the `SpatialMixingLane` reads the audio view (listener pose + a snapshot of
every source), computes distance attenuation and stereo pan, and mixes into the buffer the
CPAL callback drains. The mix runs at frame rate; the callback runs at the device sample
rate, decoupled through the mix bus. See [Audio](../concepts/audio.md) for the model.

## Expected result

The footstep grows louder as the listener approaches and pans left/right as it moves past;
a 2D music source plays at constant volume regardless of position.

## Related

- [Load and reference assets](./load-assets.md) — obtain the `SoundData` handle.
- [Spawn entities and move them](./spawn-and-transform.md) — move the listener each frame.
- [Audio](../concepts/audio.md) — the mixer, the mix bus, and the GORNA source budget.
