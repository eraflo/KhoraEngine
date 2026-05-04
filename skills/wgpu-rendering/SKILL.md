---
name: wgpu-rendering
description: "wgpu 28.0 GPU rendering — render passes, shadow mapping, PBR shaders, pipeline management, texture/buffer resources, Vulkan validation, and frame lifecycle (begin_frame/end_frame). Use for any GPU, shader, or rendering task."
license: Apache-2.0
allowed-tools: cargo-build cargo-test
metadata:
  author: eraflo
  version: "1.0.0"
  category: rendering
---

# wgpu Rendering

## Instructions

When working on the GPU rendering subsystem:

1. **Backend location**: `crates/khora-infra/src/graphics/wgpu/`
   - `system.rs` — `WgpuRenderSystem`: frame lifecycle, swapchain management
   - `device.rs` — `WgpuDevice`: resource creation, command submission, GPU polling
   - `context.rs` — `WgpuGraphicsContext`: surface/device/queue ownership
   - `command.rs` — `WgpuCommandEncoder`: render pass encoding
   - `backend.rs` — adapter selection, backend preference

2. **Frame lifecycle** (single acquire per frame):
   - `begin_frame()` — poll + wait_for_last_submission + acquire swapchain texture
   - N × `render_with_encoder()` — encode commands to the shared swapchain target
   - `end_frame()` — present + update stats
   - Called from SDK `handle_frame()` in `crates/khora-sdk/src/lib.rs`

3. **Abstract resource IDs** — all GPU resources use typed IDs:
   - `TextureId`, `TextureViewId`, `BufferId`, `PipelineId`, `BindGroupId`, etc.
   - Created/destroyed through `WgpuDevice` methods
   - Never expose raw `wgpu::Texture` in public APIs

4. **Shaders** (WGSL): `crates/khora-lanes/src/render_lane/shaders/`
   - `lit_forward.wgsl` — PBR lit shader with shadow sampling (PCF 3×3)
   - `shadow_depth.wgsl` — depth-only shadow pass
   - `simple_unlit.wgsl` — basic unlit material

5. **Shadow system**:
   - `ShadowPassLane` — renders depth to a 2048×2048 shadow atlas (4 layers)
   - `calculate_shadow_view_proj()` — directional/spot/point light matrices
   - Texel snapping prevents shadow shimmer on camera movement
   - Shadow atlas: `Depth32Float`, comparison sampler

6. **wgpu version**: 28.0 — uses `PollType::Wait` with `SubmissionIndex`

7. **Vulkan validation**: Must never produce validation errors. Common pitfalls:
   - Semaphore must be unsignaled before `get_current_texture()`
   - `base_array_layer` must be correct for texture array views
   - Command buffers must be submitted before present

## Key Types

| Type | Crate | Purpose |
|------|-------|---------|
| `WgpuRenderSystem` | khora-infra | Frame lifecycle, swapchain |
| `WgpuDevice` | khora-infra | Resource management, submission |
| `WgpuCommandEncoder` | khora-infra | Render pass encoding |
| `RenderSystem` trait | khora-core | Abstract render system interface |
| `GraphicsDevice` trait | khora-core | Abstract device interface |
| `CommandEncoder` trait | khora-core | Abstract encoder interface |
