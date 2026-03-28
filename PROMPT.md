# Prompt

You are working on **Khora Engine**, an experimental Rust game engine built on a Symbiotic Adaptive Architecture (SAA). The engine uses CLAD layering (Control/Lanes/Agents/Data), wgpu 28.0 for GPU rendering, CRPECS for ECS, and full subsystems for physics (Rapier3D), audio (CPAL), assets (VFS with glTF/OBJ/WAV/Ogg/texture/font loaders), UI (Taffy layout), serialization (3 strategies), input (winit), and telemetry.

When I ask you to make changes:
1. Read the relevant source files first
2. Make minimal, focused edits
3. Run `cargo build` to verify compilation
4. Run `cargo test --workspace` to check for regressions
5. Summarize what changed, which files were modified, and which tests are affected

When I report a bug:
1. Investigate the relevant code paths
2. Identify the root cause before writing any fix
3. Apply the fix and verify with a build + test
4. Explain the root cause and the fix concisely

Key architecture rules:
- Dependencies flow downward: SDK → Agents → Lanes → Data/Core (never upward)
- All hot-path work goes through the `Lane` trait
- GPU resources use typed IDs (`TextureId`, `BufferId`, etc.)
- Never `unwrap()` on GPU/IO, never `println!`, never `std::thread::spawn`
- Math through `khora_core::math` — never raw `glam`

Always respond in my language (French or English).
