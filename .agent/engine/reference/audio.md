# Audio — reference

Domain knowledge for Khora audio. Consulted during Research (dispatch the `codebase-*` subagents to
apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §6 (subsystem boundaries).

## Scope
Spatial 3D audio mixing, the RT-thread mix bus boundary, device/stream lifecycle, decoding, and the
`AudioSource` / `AudioListener` components.

## Key files
- Traits: `crates/khora-core/src/audio/` (`AudioDevice`, `AudioMixBus`, `AudioStream`, `StreamInfo`).
- Backend: `crates/khora-infra/src/audio/cpal/` (`CpalAudioDevice`); the mixer contract is `crates/khora-core/src/audio/mix_bus.rs` (`AudioMixBus`), its default implementation `crates/khora-infra/src/audio/default_mix_bus.rs` (`DefaultMixBus`).
- Lanes: `crates/khora-lanes/src/audio/` (`SpatialMixingLane`: 8 / 32 / 128 voices; `AudioFamily`).
  Registration: `crates/khora-lanes/src/audio/registration.rs` (TRANSFORM).
- Decoding: `crates/khora-io/src/asset/decoders/audio/` (`SymphoniaDecoder`); data `SoundData`.

## Hard rules
- Route audio through `AudioDevice` + the spatial mixing lane. **Never** call CPAL directly from agents.
- The **mix bus is the sole synchronization boundary** between audio lanes (main thread) and the hardware
  callback (RT thread). Do not block, allocate, or panic on the RT path.
- The `audio` family's agent is the one `LaneAgent`, choosing the voice level that fits its envelope — no
  per-frame state, no new agent type.
- No `unwrap()` on device open / stream errors — log and degrade gracefully (see `sandbox` `main.rs` audio setup).

Audio goes through the `AudioMixBus` (the RT-thread boundary). Use codegraph to map the device / stream /
mixing-lane wiring before editing — confirm the data path in code rather than assuming it.

## Skills
- [`add-a-lane`](../skills/add-a-lane/SKILL.md) — add an audio lane / strategy.
- [`add-a-component`](../skills/add-a-component/SKILL.md) — add an audio component.
- [`run-the-engine`](../skills/run-the-engine/SKILL.md) · [`build-and-test`](../skills/build-and-test/SKILL.md) — verify (a clean `cargo run -p sandbox`).
