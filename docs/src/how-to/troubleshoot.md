# Troubleshoot a Khora project

**Goal:** find the cause of a concrete failure fast. Each entry below names a *symptom*, the
*likely cause*, and the *fix*, grouped by subsystem. When you need the underlying mechanism rather
than the recipe, follow the link out to the concept chapter — this page stays task-oriented.

Jump to the subsystem you are fighting:

1. [Build & toolchain](#build--toolchain)
2. [Rendering / GPU (wgpu)](#rendering--gpu-wgpu)
3. [Physics](#physics)
4. [Audio](#audio)
5. [Assets / VFS](#assets--vfs)
6. [GORNA / adaptation](#gorna--adaptation)

---

## Build & toolchain

### The toolchain is rejected — "package requires rustc 1.91" (or a similar MSRV error)

**Cause.** The workspace pins its minimum supported Rust version to **1.91** in the root
`Cargo.toml` (`[workspace.package] rust-version = "1.91"`), the lowest stable toolchain that
compiles the whole tree. An older toolchain cannot build it.

**Fix.** `rustup update` (or `rustup install 1.91 && rustup default 1.91`). The MSRV is verified
with `cargo +1.91 check --workspace --all-features --all-targets`.

### `target/` grows very large

**Cause.** Expected for a workspace this size — but Khora already trims it: dev builds use
`debug = "line-tables-only"` (stack traces with file + line, no per-symbol type tables) and strip
debuginfo from dependencies, saving roughly 30–50% of `target/` size per profile. You still get
panic backtraces and `RUST_BACKTRACE=1`.

**Fix.** `cargo clean` reclaims everything. If you need to step through values in a debugger,
temporarily set `debug = 2` in a local `Cargo.toml` override rather than committing it. Release
artifacts are larger because `[profile.release]` enables `lto` and `codegen-units = 1`.

### `cargo deny check` fails in CI or locally

**Cause.** The supply-chain gate (`deny.toml`, EmbarkStudios `cargo-deny`) runs four checks:
*advisories* (RustSec security advisories — real vulnerabilities **fail**), *licenses* (only the
permissive licenses listed in `deny.toml` are allowed), *bans* (duplicate versions only **warn**;
wildcard versions on external crates are denied), and *sources* (only crates.io and explicitly
allowed sources are trusted).

**How to read the output.** `cargo deny check` prints one block per finding with the check name,
the crate, and the advisory or license ID. An `error` breaks the gate; a `warning` does not.

**Known, ignored advisory.** **RUSTSEC-2025-0141** (bincode 2.x flagged *unmaintained*) is
acknowledged in `deny.toml`'s `[advisories] ignore` list. It is an unmaintained notice, not a
security vulnerability, and the advisory itself states no safe upgrade exists — so it is expected to
be silent. Never add an ID to `ignore` to mask a live vulnerability; each entry must carry a
documented justification.

### Platform note — the default UUID is path-derived, not OS-derived

The asset index normalizes relative paths to forward slashes before deriving the default UUID, so a
path that hashes to one UUID on Windows (`textures\wood.png`) produces the *same* UUID on Linux/macOS
— assets built on one OS resolve on another. (Frozen identities in the registry are OS-agnostic too:
the stored path is always forward-slash.) See [Assets / VFS](#assets--vfs). For the supported build/run
targets, follow the [SDK quickstart](../tutorials/your-first-game.md) and the editor's Build Game flow
([Editor](../reference/editor.md)).

---

## Rendering / GPU (wgpu)

The render system classifies every surface-acquire outcome and every device-health flag into a
deliberate action, so most GPU hiccups are handled without a panic. The pure decision logic lives in
`crates/khora-infra/src/graphics/wgpu/resilience.rs`; the side effects (reconfigure / skip / error)
are applied in `system.rs`. Knowing which bucket a log line falls into tells you whether to act.

### Brief flicker or a single dropped frame after an alt-tab, resolution change, or display switch — then it recovers on its own

**Cause.** The surface reported `Lost` or `Outdated`. With a valid (non-zero) window size the system
reconfigures the swapchain and retries the acquire **within the same frame**; you may see at most a
one-frame hitch.

**Fix.** None needed — this is the resilience path doing its job. If it logs at `debug`, that is
expected.

### Nothing renders while the window is minimized or occluded, then resumes when restored

**Cause.** A minimized window has a zero-size surface; a hidden window reports `Occluded`. Both, plus
a transient `Timeout`, classify as *skip this frame* — the frame is dropped quietly (a `debug`-level
line at most) and retried next frame, instead of spamming a hard error every tick.

**Fix.** None needed. The engine resumes presenting when the window is shown again.

### A one-off `Validation` or unknown surface-acquire failure logs at `error`, but the loop keeps running

**Cause.** These classify as *non-fatal*: the frame is skipped and the error is reported upward, but
the host stays alive. A validation error is also caught by the device error handler.

**Fix.** If it recurs every frame, treat it as a real bug — capture the validation message (see
below) and inspect the offending pass.

### `EngineCore: begin_frame fatal error: …` / `… end_frame fatal error: …` and drawing stops — but no panic

**Cause.** The wgpu device error callbacks raised a *device lost* (driver crash/reset/destroy or an
internal device error) or *out-of-memory* flag. These flags are **sticky** — the device cannot
recover them in place — so `RenderError::DeviceLost` / `RenderError::DeviceOutOfMemory` is classified
as fatal (`RenderError::is_fatal()`). The render system checks device health before each submission
and, once a fatal condition is observed, stops acquiring and submitting GPU work; the engine logs it
loudly at `error` rather than crashing mid-frame so the host can tear down cleanly.

**Fix.** Device loss usually means a GPU driver crash, a driver update, or the GPU being reset by the
OS — restart the application (and update the driver if it persists). Out-of-memory means a real
VRAM/resource budget breach: reduce texture/buffer footprint or render at a lower resolution. Neither
is a code panic to debug in a backtrace; read the fatal log line.

### Vulkan validation errors in the console

**Cause.** The engine targets a clean frame loop: running `cargo run -p sandbox` should produce
**no** Vulkan validation errors. A validation message names the failing object and the rule it
broke; read it top-down.

**Fix.** Treat any validation error as a bug to fix, not noise. Reproduce under the same backend,
capture the full message, and trace it to the pass that recorded the offending command. See
[Rendering](../concepts/rendering.md).

### Black screen / nothing renders, no errors

**Cause.** Common causes, in rough order of likelihood:

- **No active camera** — without a camera there is no view/projection to draw from.
- **No light** — under a lit strategy an unlit scene can read as black; confirm a light exists or
  switch to an unlit strategy to isolate it.
- **Mesh/material not uploaded or not referenced** — entities must carry a valid mesh and material
  handle that resolved through the asset pipeline (see [Assets / VFS](#assets--vfs)).

**Fix.** Verify a camera, a light, and at least one entity with resolved mesh + material handles are
present in the world. See [Rendering](../concepts/rendering.md).

---

## Physics

The simulation runs on a **fixed timestep** decoupled from the display rate. The scheduler
(`crates/khora-control/src/scheduler.rs`, `compute_sim_steps`) accumulates real frame time and runs
whole fixed sub-steps; the `PhysicsAgent` (`crates/khora-agents/src/physics_agent/agent.rs`) owns the
fixed step via its GORNA strategy. See [GORNA](../concepts/gorna.md) and
[Profile performance](./profile-performance.md).

### A body does not move under gravity or forces

**Cause.** Either:

- The entity has **no `RigidBody`** (or it is fixed/kinematic rather than dynamic), so the provider
  never integrates it.
- **The simulation is not stepping.** Physics only steps in an engine mode that runs the
  `PhysicsAgent`. In the editor, physics runs in *Playing* mode, not while editing — see
  [Editor — Play mode](../reference/editor.md).

**Fix.** Confirm the entity carries a dynamic `RigidBody`, and that you are in a mode where the
physics agent is active.

### Fast bodies pass through thin colliders (tunneling)

**Cause.** A body moving far enough in one fixed step to skip over a thin collider is classic
discrete-step tunneling, made worse by a coarse fixed step (the LowPower strategy steps at 30 Hz;
Balanced 60 Hz; HighPerformance 120 Hz — see `apply_budget` in the physics agent).

**Fix.** Use thicker colliders for fast objects, or favour a finer fixed step. Continuous collision
detection is a provider-level concern — see [Physics](../concepts/physics.md).

### Simulation slows to slow-motion under a heavy hitch (but never freezes)

**Cause.** The **spiral-of-death guard**, working as designed. The real frame delta fed into the
accumulator is clamped to `MAX_FRAME_DELTA_SECONDS` (0.25 s), and the number of fixed sub-steps run
in one frame is capped at `MAX_SIM_STEPS` (5). When a long stall (a debugger break, an asset hitch, a
window drag) would demand more catch-up steps than the cap, the excess accumulated time is
**dropped**: the simulation runs in slow-motion for a moment rather than trying to replay seconds of
physics in one frame and freezing.

**Fix.** Nothing in normal operation — recovery is automatic once frame pacing returns. The two knobs
(`MAX_FRAME_DELTA_SECONDS`, `MAX_SIM_STEPS`) are constants in `scheduler.rs`; raise them only if you
deliberately want more catch-up at the cost of a worse worst-case frame.

### NaN positions / the simulation explodes

**Cause.** Usually a degenerate input: a zero or non-finite mass, an inverted/degenerate collider, an
enormous force, or a fixed step too large for the configured stiffness. A NaN propagates through the
integrator and corrupts transforms.

**Fix.** Validate masses and collider extents at spawn, clamp applied forces, and prefer a finer
fixed step for stiff constraints. See [Physics](../concepts/physics.md).

---

## Audio

Audio runs through the `AudioDevice` trait (CPAL backend) and the spatial mixing lane
(`crates/khora-lanes/src/audio_lane/`). As with physics, the `AudioAgent` only runs in a mode that
enables it (e.g. *Playing* in the editor).

### No spatialization — sound plays but is not panned by position

**Cause.** The mixing lane only pans when a **listener transform** is present; with no listener the
channels stay balanced (see `spatial_mixing_lane.rs`).

**Fix.** Ensure the scene has a listener (an `AudioListener` on an entity with a transform), typically
on the active camera or the player.

### No sound at all

**Cause.** Check, in order:

- The **audio agent is running** — you are in a mode that enables audio (not editor-edit mode).
- The **source is actually playing** — it has audio data and is in a playing state, not stopped or at
  zero gain.
- A **CPAL output device exists and opened** — on a headless CI box or a machine with no default
  output, the device may fail to open. A stream-level failure is reported through the CPAL error
  callback as `audio stream error: …` (`backends/cpal/device.rs`).

**Fix.** Confirm a working default output device, a playing source with non-zero gain, and an active
audio mode. See [Audio](../concepts/audio.md).

### A corrupt or unsupported audio file does not crash the engine

**Cause.** Decoders return a `Result`; a file that fails to decode is logged and skipped, not
unwrapped. This is the same contract every decoder follows (see [Assets / VFS](#assets--vfs)).

**Fix.** If a clip is silent, check the logs for a decode failure and re-export the asset in a
supported format.

---

## Assets / VFS

Assets are identified by a stable **`AssetUUID`**: `AssetUUID::new_v5(rel_path)` derived from the
forward-slash relative path under `assets/` by **default**, or a value **frozen** in the project's
identity registry (`<project>/.khora/asset-registry.ron`) once the asset has been renamed/moved in
the editor (see `crates/khora-io/src/asset/index_builder.rs` and
[File formats — asset identity registry](../reference/formats.md#asset-identity-registry)). Both dev
(FileLoader) and a release pack (PackLoader) resolve through the same registry, so a file yields the
same UUID in either — that identity is what makes dev/release transparent.

### "Asset not found" / a handle never resolves

**Cause.** For an asset that has never been renamed, its UUID is computed from the **relative path
with forward slashes**, e.g. `textures/wood.png`. A mismatch is almost always a path/identity
mismatch: the file is outside the project's `assets/` root, or a reference was authored against a
different path. (Renaming or moving *inside the editor* does **not** cause this — the registry freezes
the UUID; see the entry below for renames done outside the editor.) The default UUID is
platform-agnostic — `textures\wood.png` on Windows and `textures/wood.png` on Linux hash to the
*same* UUID — so a missing asset is a path/identity problem, not an OS path-separator problem.

**Fix.** Confirm the file lives under `<project>/assets/`, and that the reference resolves to an
indexed asset — either at its original path (unfrozen default) or at whatever path the identity
registry currently binds its UUID to.

### A reference broke after renaming/moving an asset *outside* the editor (shell, `git`, another tool)

**Cause.** Stable identity relies on the **editor** mediating the file operation: it moves the file
*and* freezes the UUID into `.khora/asset-registry.ron` in the same step. Renaming or moving an asset
from a shell, `git`, or any external tool bypasses that freeze. The file now resolves to
`new_v5(new_path)` (a *different* UUID), while scenes and materials still reference the old UUID — so
the reference orphans, exactly as it would have before the registry existed.

**Fix.** Prefer doing renames/moves in the editor's asset browser, which keeps references intact. If a
file was already moved outside the editor, either move it back to its original path (restoring the
default UUID) or add a matching entry to the registry so the old UUID binds to the new path.

### An asset loads in the editor (FileLoader) but not in a built game (PackLoader), or vice versa

**Cause.** Both loaders key by the same UUID, so an asset that resolves in one should resolve in the
other *if it was included*. The usual cause is that the file was not under `assets/` when the pack was
built, or it was a scratch/temporary file the scan skips. The index builder ignores OS/editor scratch
files: names starting with `.` or `~`, and files ending in `.tmp`, `.swp`, `.bak`, or `~`.

**Fix.** Keep referenced assets under `assets/` with non-scratch names, and rebuild the pack. See
[Editor — Build Game](../reference/editor.md) and [Assets](../concepts/assets.md).

### Hot-reload does not fire when I edit a file

**Cause.** The filesystem watcher is timing-dependent — it reacts to OS file events, which can be
delayed, coalesced, or (for some editors that write via atomic-rename to a temp file) hidden behind a
name the scan ignores.

**Fix.** Save again, or save in place rather than via a `.tmp`/`.swp` shadow file. Hot-reload is a
convenience; a reload-on-demand always works.

### A corrupt asset cannot crash the engine

**Cause.** Every decoder returns a `Result`, and the index builder reads file contents behind a
fallible path. A malformed file yields an error (or empty dependencies for a corrupt material), not a
panic — see the `corrupt_material_still_builds_with_empty_deps` behaviour in `index_builder.rs`.

**Fix.** Check the logs for the decode warning naming the file, then re-export it.

---

## GORNA / adaptation

The DCC adapts each agent's strategy every cold-path tick based on hardware and frame-time pressure.
When quality changes unexpectedly, GORNA is usually responding to a real signal. The mechanism is in
[GORNA](../concepts/gorna.md); the code is `khora-control/src/analysis.rs` (`HeuristicEngine`) and
`khora-control/src/service.rs` (`DccService`). To investigate a single decision frame by frame, use
[Debug a frame](./debug-a-frame.md).

### "Why did quality suddenly drop?" (a strategy downgraded on its own)

**Cause.** The DCC's heuristics shape a single frame-time **target**, and a **PID controller** drives
the `global_budget_multiplier` that scales every agent's budget. Any of these will lower the
multiplier and push agents to cheaper strategies:

- **Thermal** — a throttling GPU/CPU relaxes the target to 30 FPS; `Critical` triggers a hard ~20 FPS
  safety cap.
- **Battery** — `Low` relaxes the target to 30 FPS and prefers LowPower; `Critical` caps to ~20 FPS.
- **Frame-time / stutter / trend** — frames over target, high variance, or a degrading slope tighten
  the budget; the cost-model forecast can tighten it *before* a breach.
- **CPU / GPU / memory pressure** — load above the critical threshold forces negotiation.
- **Death spiral** — three or more independent pressure sources active at once trip an emergency stop.

**Fix.** This is adaptive behaviour, not a bug. To diagnose, watch the editor's **GORNA Stream**
panel — it prints the reason for each switch (e.g. "RenderAgent: LitForward → Forward+ — GPU
pressure"). See [Pin a strategy](#pin-a-strategy-so-adaptation-stops-surprising-you) below.

### Quality keeps oscillating, or upgrades right after a downgrade

**Cause.** The loop is closed in both directions: once measured frame time settles under the
setpoint, the PID multiplier climbs back toward 1.0 and budgets are re-issued so agents upgrade
again. Re-arbitration fires when the multiplier moves by more than `PID_RENEGOTIATE_DELTA` (0.05).
Near a threshold this can cycle.

**Fix.** Pin or clamp the agent (below) to stop the oscillation while you investigate.

### Pin a strategy so adaptation stops surprising you

**Cause.** You want to remove adaptation as a variable while reproducing a behaviour.

**Fix.** Set the agent's **`AdaptationMode`** through the DCC service
(`DccService::set_adaptation_mode(agent_id, mode)`):

- `Manual(strategy)` — pin the agent to one strategy; GORNA will not move it (a death-spiral safety
  stop can still force a downgrade).
- `Stable` — block opportunistic *up*-switches (it can still downgrade under pressure).
- `Bounded { min, max }` — clamp the negotiated range.
- `Learning` (default) — negotiate freely.

These are the four `AdaptationMode` variants enforced today. For the full recipe — including how to
read which agent chose which strategy and why — see [Debug a frame](./debug-a-frame.md).

### Reproduce a specific adaptation decision

**Cause.** You need the *exact* sequence of strategy choices to recur bit-for-bit (QA, a flaky bug,
lockstep).

**Fix.** Pin the relevant agent with `Manual(strategy)` to remove adaptation as a variable, then read
the live decisions in the GORNA Stream panel. For deterministic capture-and-replay of the whole
arbitration trace, the DCC also exposes a decision recorder
(`DccService::start_decision_recording` → `stop_decision_recording` → `replay_decisions`) — see
[Debug a frame](./debug-a-frame.md) for how to drive it.

---

*See also: [GORNA](../concepts/gorna.md), [Telemetry](../concepts/telemetry.md),
[Profile performance](./profile-performance.md), [Debug a frame](./debug-a-frame.md),
[Rendering](../concepts/rendering.md), [Physics](../concepts/physics.md), [Audio](../concepts/audio.md),
[Assets](../concepts/assets.md), [Editor](../reference/editor.md).*
