---
name: run-the-engine
description: Launches the engine to see a change running — the editor or the sandbox demo. Use when asked to run, start, or visually confirm the engine, or to check a render/gameplay change in the real app (not just tests).
---

# Run the engine

| Target | Command | Use for |
|---|---|---|
| **Sandbox** (demo game) | `cargo run -p sandbox` | Render/gameplay checks, frame-loop validation. Primary. |
| **Editor** | `cargo run -p khora-editor` | Editor panels, gizmos, scene authoring, grid. |
| Runtime player | `cargo run -p khora-runtime` | Packed-asset player (needs a built pack). |
| Hub | `cargo run -p hub` | Project manager / launcher. |

## Sandbox controls
Right-mouse + drag = look; WASD = move; Space = up; Shift = down.

## What to confirm
- Window opens, scene renders (sandbox spawns a plane, sun light, point light, and colored spheres).
- **No Vulkan validation errors** in the log, clean frame loop.
- For editor: panels dock, gizmos draw, the grid shows (editor enables `GridConfig`; sandbox leaves it off).

Logging is `env_logger` (`RUST_LOG=info` by default). If the app fails to start, report the actual error;
don't claim success you didn't observe.
