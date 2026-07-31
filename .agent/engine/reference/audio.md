# Audio — reference

Domain knowledge for Khora audio. Consulted during Research (dispatch the `codebase-*` subagents to
apply it to concrete files). Follow [`../RULES.md`](../RULES.md) §6 (subsystem boundaries).

## Scope
Spatial 3D audio mixing, the RT-thread mix bus boundary, device/stream lifecycle, decoding, and the
`AudioSource` / `AudioListener` components.

## Key files
- Traits: `crates/khora-core/src/audio/` (`AudioDevice`, `AudioMixBus`, `AudioStream`, `DefaultMixBus`, `StreamInfo`).
- Backend: `crates/khora-infra/src/audio/backends/cpal/` (`CpalAudioDevice`, `mix_bus.rs`).
- Lanes: `crates/khora-lanes/src/audio_lane/` (SpatialMixing, SourceUpdate).
- Decoding: `crates/khora-io/src/asset/decoders/audio/` (`SymphoniaDecoder`); data `SoundData`.

## Hard rules
- Route audio through `AudioDevice` + the spatial mixing lane. **Never** call CPAL directly from agents.
- The **mix bus is the sole synchronization boundary** between audio lanes (main thread) and the hardware
  callback (RT thread). Do not block, allocate, or panic on the RT path.
- The `AudioAgent` stays a strategist (source count / quality) — no per-frame state.
- No `unwrap()` on device open / stream errors — log and degrade gracefully (see `sandbox` `main.rs` audio setup).

Audio goes through the `AudioMixBus` (the RT-thread boundary). Use codegraph to map the device / stream /
mixing-lane wiring before editing — confirm the data path in code rather than assuming it.

## Skills
- [`add-a-lane`](../skills/add-a-lane/SKILL.md) — add an audio lane / strategy.
- [`add-a-component`](../skills/add-a-component/SKILL.md) — add an audio component.
- [`run-the-engine`](../skills/run-the-engine/SKILL.md) · [`build-and-test`](../skills/build-and-test/SKILL.md) — verify (clean `cargo run -p sandbox`).
