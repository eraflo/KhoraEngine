# Daily Log

## 2026-03-28

### Session Summary
- Fixed Vulkan semaphore validation errors (VUID-vkAcquireNextImageKHR-semaphore-01286)
  - Root cause: 2 acquire-submit-present cycles per visual frame (RenderAgent + UiAgent)
  - Fix: Added `begin_frame()`/`end_frame()` to `RenderSystem` trait, single swapchain acquire per frame
  - Files: `render_system.rs`, `system.rs`, `lib.rs`
- Fixed shadow jittering when moving camera
  - Root cause: directional light shadow ortho projection shifted sub-texel amounts
  - Fix: Texel snapping in `calculate_shadow_view_proj()`
  - File: `shadow_pass_lane.rs`
- Set up GitAgent configuration for the project
