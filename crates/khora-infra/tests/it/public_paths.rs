// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Compile-level guard over `khora_infra`'s public surface, plus the exported
//! macros and the paths other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), and the
//! trait impls. A reorganisation that moves code between files must keep every
//! one of these paths valid, so this module stops compiling the moment one
//! disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions).
//!
//! The list was generated from `khora_infra`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_app_context<T: khora_core::ui::AppContext>() {}
fn is_audio_device<T: khora_core::audio::AudioDevice>() {}
fn is_audio_mix_bus<T: khora_core::audio::AudioMixBus>() {}
fn is_audio_stream<T: khora_core::audio::AudioStream>() {}
fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_editor_overlay<T: khora_core::ui::EditorOverlay>() {}
fn is_editor_shell<T: khora_core::ui::EditorShell>() {}
fn is_global_alloc<T: std::alloc::GlobalAlloc>() {}
fn is_hardware_monitor<T: khora_core::platform::HardwareMonitor>() {}
fn is_has_display_handle<T: raw_window_handle::HasDisplayHandle>() {}
fn is_has_window_handle<T: raw_window_handle::HasWindowHandle>() {}
fn is_iterator<T: Iterator>() {}
fn is_khora_window<T: khora_core::platform::KhoraWindow>() {}
fn is_layout_system<T: khora_core::ui::LayoutSystem>() {}
fn is_physics_provider<T: khora_core::physics::PhysicsProvider>() {}
fn is_pipeline_system<T: khora_core::renderer::traits::PipelineSystem>() {}
fn is_pod<T: bytemuck::Pod>() {}
fn is_render_system<T: khora_core::renderer::RenderSystem>() {}
fn is_resource_monitor<T: khora_core::telemetry::ResourceMonitor>() {}
fn is_send<T: Send>() {}
fn is_sync<T: Sync>() {}
fn is_text_layout<T: khora_core::renderer::api::text::TextLayout>() {}
fn is_text_renderer<T: khora_core::renderer::api::text::TextRenderer>() {}
fn is_ui_builder<T: khora_core::ui::UiBuilder>() {}
fn is_zeroable<T: bytemuck::Zeroable>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_infra::audio as _;
    use khora_infra::audio::cpal as _;
    use khora_infra::graphics as _;
    use khora_infra::graphics::shader as _;
    use khora_infra::graphics::text as _;
    use khora_infra::graphics::text::custom as _;
    use khora_infra::graphics::text::custom::pixel_font as _;
    use khora_infra::graphics::text::standard as _;
    use khora_infra::graphics::wgpu as _;
    use khora_infra::memory as _;
    use khora_infra::physics as _;
    use khora_infra::physics::khora as _;
    use khora_infra::physics::khora::collision as _;
    use khora_infra::physics::khora::dynamic_tree as _;
    use khora_infra::physics::khora::solver as _;
    use khora_infra::physics::rapier as _;
    use khora_infra::platform as _;
    use khora_infra::platform::sysinfo_impl as _;
    use khora_infra::platform::winit as _;
    use khora_infra::platform::winit::input as _;
    use khora_infra::platform::winit::window as _;
    use khora_infra::telemetry as _;
    use khora_infra::telemetry::gpu_monitor as _;
    use khora_infra::telemetry::memory_monitor as _;
    use khora_infra::telemetry::vram_monitor as _;
    use khora_infra::ui as _;
    use khora_infra::ui::egui as _;
    use khora_infra::ui::egui::app as _;
    use khora_infra::ui::egui::overlay as _;
    use khora_infra::ui::egui::renderer as _;
    use khora_infra::ui::egui::shell as _;
    use khora_infra::ui::egui::theme as _;
    use khora_infra::ui::egui::ui_builder as _;
    use khora_infra::ui::taffy as _;
    use khora_infra::ui::taffy::taffy_layout as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn contact_manifold_fields(x: &khora_infra::physics::khora::ContactManifold) {
    let _ = (&x.normal, &x.depth, &x.point);
}

fn dynamic_tree_node_fields(x: &khora_infra::physics::khora::dynamic_tree::DynamicTreeNode<u32>) {
    let _ = (&x.aabb, &x.user_data, &x.parent, &x.children, &x.height);
}

fn impulse_solver_fields(x: &khora_infra::physics::khora::solver::ImpulseSolver) {
    let _ = (&x.restitution, &x.baumgarte_percent, &x.slop);
}

fn velocity_state_fields(x: &khora_infra::physics::khora::solver::VelocityState) {
    let _ = (
        &x.linear_velocity,
        &x.angular_velocity,
        &x.mass,
        &x.body_type,
    );
}

fn standard_text_layout_fields(x: &khora_infra::graphics::text::standard::StandardTextLayout) {
    let _ = (
        &x.size,
        &x.glyph_positions,
        &x.font_handle,
        &x.font_uuid,
        &x.font_size,
    );
}

fn text_vertex_fields(x: &khora_infra::graphics::text::standard::TextVertex) {
    let _ = (&x.pos, &x.uv, &x.color);
}

fn window_config_input_fields(x: &khora_infra::ui::egui::app::WindowConfigInput) {
    let _ = (&x.title, &x.width, &x.height, &x.icon);
}

fn window_icon_input_fields(x: &khora_infra::ui::egui::app::WindowIconInput) {
    let _ = (&x.rgba, &x.width, &x.height);
}

fn egui_frame_render_state_fields(x: &khora_infra::ui::egui::overlay::EguiFrameRenderState) {
    let _ = (
        &x.graphics_context,
        &x.encoder,
        &x.target_view,
        &x.width_px,
        &x.height_px,
    );
}

fn egui_render_state_fields(x: &khora_infra::ui::egui::renderer::EguiRenderState<'static>) {
    let _ = (
        &x.device,
        &x.queue,
        &x.encoder,
        &x.target_view,
        &x.width_px,
        &x.height_px,
    );
}

#[test]
fn module_crate_root_paths_still_resolve() {
    let _ = type_name::<khora_infra::graphics::wgpu::WgpuRenderSystem>();
    let _ = type_name::<khora_infra::WgpuRenderSystem>();
    same_type(
        PhantomData::<khora_infra::graphics::wgpu::WgpuRenderSystem>,
        PhantomData::<khora_infra::WgpuRenderSystem>,
    );
    let _ = khora_infra::WgpuRenderSystem::new;
    let _ = khora_infra::WgpuRenderSystem::create_viewport_target;
    let _ = khora_infra::WgpuRenderSystem::render_viewport_clear;
    let _ = khora_infra::WgpuRenderSystem::viewport_size;
    let _ = khora_infra::WgpuRenderSystem::create_editor_overlay;
    let _ = khora_infra::WgpuRenderSystem::create_editor_overlay_and_shell;
    is_debug::<khora_infra::WgpuRenderSystem>();
    is_default::<khora_infra::WgpuRenderSystem>();
    is_render_system::<khora_infra::WgpuRenderSystem>();
    is_send::<khora_infra::WgpuRenderSystem>();
    is_sync::<khora_infra::WgpuRenderSystem>();
    let _ = type_name::<khora_infra::DefaultMixBus>();
    same_type(
        PhantomData::<khora_infra::DefaultMixBus>,
        PhantomData::<khora_infra::audio::DefaultMixBus>,
    );
    let _ = type_name::<khora_infra::SaaTrackingAllocator<std::alloc::System>>();
    same_type(
        PhantomData::<khora_infra::SaaTrackingAllocator<std::alloc::System>>,
        PhantomData::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>,
    );
}

#[test]
fn module_audio_paths_still_resolve() {
    let _ = type_name::<khora_infra::audio::cpal::CpalAudioDevice>();
    let _ = khora_infra::audio::cpal::CpalAudioDevice::new;
    is_default::<khora_infra::audio::cpal::CpalAudioDevice>();
    is_audio_device::<khora_infra::audio::cpal::CpalAudioDevice>();
    let _ = type_name::<khora_infra::audio::cpal::CpalAudioStream>();
    is_send::<khora_infra::audio::cpal::CpalAudioStream>();
    is_sync::<khora_infra::audio::cpal::CpalAudioStream>();
    is_audio_stream::<khora_infra::audio::cpal::CpalAudioStream>();
    let _ = type_name::<khora_infra::audio::DefaultMixBus>();
    let _ = khora_infra::audio::DefaultMixBus::new;
    is_audio_mix_bus::<khora_infra::audio::DefaultMixBus>();
    is_send::<khora_infra::audio::DefaultMixBus>();
    is_sync::<khora_infra::audio::DefaultMixBus>();
}

#[test]
fn module_memory_paths_still_resolve() {
    let _ = type_name::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>();
    // `const fn`: a `static` global allocator is built with it.
    const _: khora_infra::memory::SaaTrackingAllocator<std::alloc::System> =
        khora_infra::memory::SaaTrackingAllocator::new(std::alloc::System);
    is_debug::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>();
    is_default::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>();
    is_clone::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>();
    is_copy::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>();
    is_global_alloc::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>();
    // The default inner allocator is `System`.
    same_type(
        PhantomData::<khora_infra::memory::SaaTrackingAllocator>,
        PhantomData::<khora_infra::memory::SaaTrackingAllocator<std::alloc::System>>,
    );
}

#[test]
fn module_graphics_paths_still_resolve() {
    let _ = type_name::<khora_infra::graphics::wgpu::WgpuPipelineSystem>();
    let _ = type_name::<khora_infra::graphics::WgpuPipelineSystem>();
    same_type(
        PhantomData::<khora_infra::graphics::wgpu::WgpuPipelineSystem>,
        PhantomData::<khora_infra::graphics::WgpuPipelineSystem>,
    );
    let _ = khora_infra::graphics::WgpuPipelineSystem::new;
    is_pipeline_system::<khora_infra::graphics::WgpuPipelineSystem>();
    let _: &str = khora_infra::graphics::shader::EGUI_WGSL;
    let _: &str = khora_infra::graphics::shader::TEXT_WGSL;
    assert_eq!(
        khora_infra::graphics::EGUI_WGSL,
        khora_infra::graphics::shader::EGUI_WGSL
    );
    assert_eq!(
        khora_infra::graphics::TEXT_WGSL,
        khora_infra::graphics::shader::TEXT_WGSL
    );
}

#[test]
fn module_physics_paths_still_resolve() {
    let _ = type_name::<khora_infra::physics::khora::ContactManifold>();
    let _ = contact_manifold_fields as fn(&khora_infra::physics::khora::ContactManifold);
    let _ = khora_infra::physics::khora::ContactManifold::inverted;
    is_debug::<khora_infra::physics::khora::ContactManifold>();
    is_clone::<khora_infra::physics::khora::ContactManifold>();
    is_copy::<khora_infra::physics::khora::ContactManifold>();
    let _ = type_name::<khora_infra::physics::khora::collision::NarrowPhase>();
    let _ = khora_infra::physics::khora::collision::NarrowPhase::new;
    let _ = khora_infra::physics::khora::collision::NarrowPhase::detect;
    is_default::<khora_infra::physics::khora::collision::NarrowPhase>();
    let _ = type_name::<khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>();
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::new;
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::insert;
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::remove;
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::update;
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::get_user_data;
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::query_pairs::<
        fn(&u32, &u32),
    >;
    let _ =
        <khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>::query::<fn(&u32) -> bool>;
    is_debug::<khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>();
    is_clone::<khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>();
    is_default::<khora_infra::physics::khora::dynamic_tree::DynamicTree<u32>>();
    let _ =
        type_name::<khora_infra::physics::khora::dynamic_tree::DynamicTreeIterator<'static, u32>>();
    is_iterator::<khora_infra::physics::khora::dynamic_tree::DynamicTreeIterator<'static, u32>>();
    let _ = type_name::<khora_infra::physics::khora::dynamic_tree::DynamicTreeNode<u32>>();
    let _ = dynamic_tree_node_fields
        as fn(&khora_infra::physics::khora::dynamic_tree::DynamicTreeNode<u32>);
    let _ = <khora_infra::physics::khora::dynamic_tree::DynamicTreeNode<u32>>::is_leaf;
    is_debug::<khora_infra::physics::khora::dynamic_tree::DynamicTreeNode<u32>>();
    is_clone::<khora_infra::physics::khora::dynamic_tree::DynamicTreeNode<u32>>();
    let _ = type_name::<khora_infra::physics::khora::solver::ImpulseSolver>();
    let _ = impulse_solver_fields as fn(&khora_infra::physics::khora::solver::ImpulseSolver);
    let _ = khora_infra::physics::khora::solver::ImpulseSolver::new;
    let _ = khora_infra::physics::khora::solver::ImpulseSolver::resolve;
    is_default::<khora_infra::physics::khora::solver::ImpulseSolver>();
    let _ = type_name::<khora_infra::physics::khora::solver::VelocityState>();
    let _ = velocity_state_fields as fn(&khora_infra::physics::khora::solver::VelocityState);
    is_debug::<khora_infra::physics::khora::solver::VelocityState>();
    is_clone::<khora_infra::physics::khora::solver::VelocityState>();
    is_copy::<khora_infra::physics::khora::solver::VelocityState>();
    let _ = type_name::<khora_infra::physics::rapier::RapierPhysicsWorld>();
    is_default::<khora_infra::physics::rapier::RapierPhysicsWorld>();
    is_physics_provider::<khora_infra::physics::rapier::RapierPhysicsWorld>();
}

#[test]
fn module_platform_paths_still_resolve() {
    let _ = khora_infra::platform::winit::input::translate_winit_input;
    let _ = type_name::<khora_infra::platform::sysinfo_impl::SysinfoMonitor>();
    let _ = khora_infra::platform::sysinfo_impl::SysinfoMonitor::new;
    let _ = khora_infra::platform::sysinfo_impl::SysinfoMonitor::refresh;
    is_hardware_monitor::<khora_infra::platform::sysinfo_impl::SysinfoMonitor>();
    is_default::<khora_infra::platform::sysinfo_impl::SysinfoMonitor>();
    let _ = type_name::<khora_infra::platform::winit::window::WinitWindow>();
    let _ = type_name::<khora_infra::platform::winit::WinitWindow>();
    let _ = type_name::<khora_infra::WinitWindow>();
    same_type(
        PhantomData::<khora_infra::platform::winit::WinitWindow>,
        PhantomData::<khora_infra::platform::winit::window::WinitWindow>,
    );
    same_type(
        PhantomData::<khora_infra::WinitWindow>,
        PhantomData::<khora_infra::platform::winit::window::WinitWindow>,
    );
    let _ = khora_infra::platform::winit::window::WinitWindow::winit_window;
    let _ = khora_infra::platform::winit::window::WinitWindow::clone_winit_arc;
    is_debug::<khora_infra::platform::winit::window::WinitWindow>();
    is_clone::<khora_infra::platform::winit::window::WinitWindow>();
    is_has_window_handle::<khora_infra::platform::winit::window::WinitWindow>();
    is_has_display_handle::<khora_infra::platform::winit::window::WinitWindow>();
    is_khora_window::<khora_infra::platform::winit::window::WinitWindow>();
    let _ = type_name::<khora_infra::platform::winit::window::WinitWindowBuilder>();
    let _ = type_name::<khora_infra::platform::winit::WinitWindowBuilder>();
    let _ = type_name::<khora_infra::WinitWindowBuilder>();
    same_type(
        PhantomData::<khora_infra::platform::winit::WinitWindowBuilder>,
        PhantomData::<khora_infra::platform::winit::window::WinitWindowBuilder>,
    );
    same_type(
        PhantomData::<khora_infra::WinitWindowBuilder>,
        PhantomData::<khora_infra::platform::winit::window::WinitWindowBuilder>,
    );
    let _ = khora_infra::platform::winit::window::WinitWindowBuilder::new;
    let _: fn(
        khora_infra::platform::winit::window::WinitWindowBuilder,
        String,
    ) -> khora_infra::platform::winit::window::WinitWindowBuilder =
        khora_infra::platform::winit::window::WinitWindowBuilder::with_title;
    let _ = khora_infra::platform::winit::window::WinitWindowBuilder::with_dimensions;
    let _ = khora_infra::platform::winit::window::WinitWindowBuilder::with_icon_rgba;
    let _ = khora_infra::platform::winit::window::WinitWindowBuilder::build;
    is_default::<khora_infra::platform::winit::window::WinitWindowBuilder>();
}

#[test]
fn module_graphics_text_paths_still_resolve() {
    let _ = khora_infra::graphics::text::custom::pixel_font::get_font_bits;
    let _ = khora_infra::graphics::text::custom::pixel_font::rasterize_glyph;
    let _ = type_name::<khora_infra::graphics::text::standard::StandardTextLayout>();
    let _ = standard_text_layout_fields
        as fn(&khora_infra::graphics::text::standard::StandardTextLayout);
    is_text_layout::<khora_infra::graphics::text::standard::StandardTextLayout>();
    let _ = type_name::<khora_infra::graphics::text::standard::StandardTextRenderer>();
    let _ = type_name::<khora_infra::graphics::text::StandardTextRenderer>();
    let _ = type_name::<khora_infra::graphics::StandardTextRenderer>();
    let _ = type_name::<khora_infra::StandardTextRenderer>();
    same_type(
        PhantomData::<khora_infra::graphics::StandardTextRenderer>,
        PhantomData::<khora_infra::graphics::text::standard::StandardTextRenderer>,
    );
    same_type(
        PhantomData::<khora_infra::graphics::text::StandardTextRenderer>,
        PhantomData::<khora_infra::graphics::text::standard::StandardTextRenderer>,
    );
    same_type(
        PhantomData::<khora_infra::StandardTextRenderer>,
        PhantomData::<khora_infra::graphics::text::standard::StandardTextRenderer>,
    );
    let _ = khora_infra::graphics::text::standard::StandardTextRenderer::new;
    is_text_renderer::<khora_infra::graphics::text::standard::StandardTextRenderer>();
    let _ = type_name::<khora_infra::graphics::text::standard::TextVertex>();
    let _ = text_vertex_fields as fn(&khora_infra::graphics::text::standard::TextVertex);
    is_copy::<khora_infra::graphics::text::standard::TextVertex>();
    is_clone::<khora_infra::graphics::text::standard::TextVertex>();
    is_debug::<khora_infra::graphics::text::standard::TextVertex>();
    is_pod::<khora_infra::graphics::text::standard::TextVertex>();
    is_zeroable::<khora_infra::graphics::text::standard::TextVertex>();
}

#[test]
fn module_telemetry_paths_still_resolve() {
    let _ = type_name::<khora_infra::telemetry::gpu_monitor::GpuMonitor>();
    let _ = type_name::<khora_infra::GpuMonitor>();
    same_type(
        PhantomData::<khora_infra::GpuMonitor>,
        PhantomData::<khora_infra::telemetry::gpu_monitor::GpuMonitor>,
    );
    let _ = khora_infra::telemetry::gpu_monitor::GpuMonitor::new;
    let _ = khora_infra::telemetry::gpu_monitor::GpuMonitor::get_gpu_report;
    let _ = khora_infra::telemetry::gpu_monitor::GpuMonitor::update_from_frame_stats;
    is_debug::<khora_infra::telemetry::gpu_monitor::GpuMonitor>();
    is_resource_monitor::<khora_infra::telemetry::gpu_monitor::GpuMonitor>();
    let _ = type_name::<khora_infra::telemetry::memory_monitor::MemoryMonitor>();
    let _ = type_name::<khora_infra::MemoryMonitor>();
    same_type(
        PhantomData::<khora_infra::MemoryMonitor>,
        PhantomData::<khora_infra::telemetry::memory_monitor::MemoryMonitor>,
    );
    let _ = khora_infra::telemetry::memory_monitor::MemoryMonitor::new;
    let _ = khora_infra::telemetry::memory_monitor::MemoryMonitor::get_memory_report;
    let _ = khora_infra::telemetry::memory_monitor::MemoryMonitor::reset_peak_usage;
    is_debug::<khora_infra::telemetry::memory_monitor::MemoryMonitor>();
    is_resource_monitor::<khora_infra::telemetry::memory_monitor::MemoryMonitor>();
    let _ = type_name::<khora_infra::telemetry::vram_monitor::VramMonitor>();
    let _ = type_name::<khora_infra::VramMonitor>();
    same_type(
        PhantomData::<khora_infra::VramMonitor>,
        PhantomData::<khora_infra::telemetry::vram_monitor::VramMonitor>,
    );
    let _ = khora_infra::telemetry::vram_monitor::VramMonitor::new;
    is_debug::<khora_infra::telemetry::vram_monitor::VramMonitor>();
    is_resource_monitor::<khora_infra::telemetry::vram_monitor::VramMonitor>();
}

#[test]
fn module_ui_paths_still_resolve() {
    let _ = type_name::<khora_infra::ui::egui::app::EguiAppContext<'static>>();
    let _ = type_name::<khora_infra::ui::egui::EguiAppContext<'static>>();
    same_type(
        PhantomData::<khora_infra::ui::egui::app::EguiAppContext<'static>>,
        PhantomData::<khora_infra::ui::egui::EguiAppContext<'static>>,
    );
    let _ = khora_infra::ui::egui::EguiAppContext::new;
    is_app_context::<khora_infra::ui::egui::EguiAppContext<'static>>();
    let _ = type_name::<khora_infra::ui::egui::app::WindowConfigInput>();
    let _ = window_config_input_fields as fn(&khora_infra::ui::egui::app::WindowConfigInput);
    is_debug::<khora_infra::ui::egui::app::WindowConfigInput>();
    is_clone::<khora_infra::ui::egui::app::WindowConfigInput>();
    is_default::<khora_infra::ui::egui::app::WindowConfigInput>();
    let _ = type_name::<khora_infra::ui::egui::app::WindowIconInput>();
    let _ = window_icon_input_fields as fn(&khora_infra::ui::egui::app::WindowIconInput);
    is_debug::<khora_infra::ui::egui::app::WindowIconInput>();
    is_clone::<khora_infra::ui::egui::app::WindowIconInput>();
    let _ = type_name::<khora_infra::ui::egui::overlay::EguiFrameRenderState>();
    let _ = type_name::<khora_infra::ui::egui::EguiFrameRenderState>();
    let _ = type_name::<khora_infra::ui::EguiFrameRenderState>();
    let _ = type_name::<khora_infra::EguiFrameRenderState>();
    same_type(
        PhantomData::<khora_infra::ui::egui::EguiFrameRenderState>,
        PhantomData::<khora_infra::ui::egui::overlay::EguiFrameRenderState>,
    );
    same_type(
        PhantomData::<khora_infra::ui::EguiFrameRenderState>,
        PhantomData::<khora_infra::ui::egui::overlay::EguiFrameRenderState>,
    );
    same_type(
        PhantomData::<khora_infra::EguiFrameRenderState>,
        PhantomData::<khora_infra::ui::egui::overlay::EguiFrameRenderState>,
    );
    let _ =
        egui_frame_render_state_fields as fn(&khora_infra::ui::egui::overlay::EguiFrameRenderState);
    let _ = type_name::<khora_infra::ui::egui::overlay::EguiOverlay>();
    let _ = type_name::<khora_infra::ui::egui::EguiOverlay>();
    let _ = type_name::<khora_infra::ui::EguiOverlay>();
    let _ = type_name::<khora_infra::EguiOverlay>();
    same_type(
        PhantomData::<khora_infra::ui::egui::EguiOverlay>,
        PhantomData::<khora_infra::ui::egui::overlay::EguiOverlay>,
    );
    same_type(
        PhantomData::<khora_infra::ui::EguiOverlay>,
        PhantomData::<khora_infra::ui::egui::overlay::EguiOverlay>,
    );
    same_type(
        PhantomData::<khora_infra::EguiOverlay>,
        PhantomData::<khora_infra::ui::egui::overlay::EguiOverlay>,
    );
    let _ = khora_infra::ui::egui::overlay::EguiOverlay::new;
    let _ = khora_infra::ui::egui::overlay::EguiOverlay::context;
    let _ = khora_infra::ui::egui::overlay::EguiOverlay::register_viewport_texture;
    let _ = khora_infra::ui::egui::overlay::EguiOverlay::update_viewport_texture;
    is_editor_overlay::<khora_infra::ui::egui::overlay::EguiOverlay>();
    is_send::<khora_infra::ui::egui::overlay::EguiOverlay>();
    is_sync::<khora_infra::ui::egui::overlay::EguiOverlay>();
    let _ = type_name::<khora_infra::ui::egui::renderer::EguiRenderState<'static>>();
    let _ =
        egui_render_state_fields as fn(&khora_infra::ui::egui::renderer::EguiRenderState<'static>);
    let _ = type_name::<khora_infra::ui::egui::renderer::EguiWgpuRenderer>();
    let _ = type_name::<khora_infra::ui::egui::EguiWgpuRenderer>();
    same_type(
        PhantomData::<khora_infra::ui::egui::EguiWgpuRenderer>,
        PhantomData::<khora_infra::ui::egui::renderer::EguiWgpuRenderer>,
    );
    let _ = khora_infra::ui::egui::renderer::EguiWgpuRenderer::new;
    let _ = khora_infra::ui::egui::renderer::EguiWgpuRenderer::initialize;
    let _ = khora_infra::ui::egui::renderer::EguiWgpuRenderer::update_textures;
    let _ = khora_infra::ui::egui::renderer::EguiWgpuRenderer::register_external_texture;
    let _ = khora_infra::ui::egui::renderer::EguiWgpuRenderer::update_external_texture;
    let _ = khora_infra::ui::egui::renderer::EguiWgpuRenderer::render;
    let _ = khora_infra::ui::egui::app::run_native::<fn() -> Box<dyn khora_core::ui::App>>;
    let _ = khora_infra::ui::egui::run_native::<fn() -> Box<dyn khora_core::ui::App>>;
    same_item(
        &khora_infra::ui::egui::app::run_native::<fn() -> Box<dyn khora_core::ui::App>>,
        &khora_infra::ui::egui::run_native::<fn() -> Box<dyn khora_core::ui::App>>,
    );
    let _ = type_name::<khora_infra::ui::egui::shell::EguiEditorShell>();
    let _ = type_name::<khora_infra::ui::egui::EguiEditorShell>();
    let _ = type_name::<khora_infra::ui::EguiEditorShell>();
    let _ = type_name::<khora_infra::EguiEditorShell>();
    same_type(
        PhantomData::<khora_infra::ui::egui::EguiEditorShell>,
        PhantomData::<khora_infra::ui::egui::shell::EguiEditorShell>,
    );
    same_type(
        PhantomData::<khora_infra::ui::EguiEditorShell>,
        PhantomData::<khora_infra::ui::egui::shell::EguiEditorShell>,
    );
    same_type(
        PhantomData::<khora_infra::EguiEditorShell>,
        PhantomData::<khora_infra::ui::egui::shell::EguiEditorShell>,
    );
    let _ = khora_infra::ui::egui::shell::EguiEditorShell::new;
    let _ = khora_infra::ui::egui::shell::EguiEditorShell::register_viewport_texture;
    let _ = khora_infra::ui::egui::shell::EguiEditorShell::resolve_viewport_texture;
    let _ = khora_infra::ui::egui::shell::EguiEditorShell::last_status;
    is_editor_shell::<khora_infra::ui::egui::shell::EguiEditorShell>();
    let _ = khora_infra::ui::egui::theme::apply_theme;
    let _ = type_name::<khora_infra::ui::egui::ui_builder::EguiUiBuilder<'static>>();
    let _ = type_name::<khora_infra::ui::egui::EguiUiBuilder<'static>>();
    let _ = type_name::<khora_infra::ui::EguiUiBuilder<'static>>();
    let _ = type_name::<khora_infra::EguiUiBuilder<'static>>();
    same_type(
        PhantomData::<khora_infra::ui::egui::EguiUiBuilder<'static>>,
        PhantomData::<khora_infra::ui::egui::ui_builder::EguiUiBuilder<'static>>,
    );
    same_type(
        PhantomData::<khora_infra::ui::EguiUiBuilder<'static>>,
        PhantomData::<khora_infra::ui::egui::ui_builder::EguiUiBuilder<'static>>,
    );
    same_type(
        PhantomData::<khora_infra::EguiUiBuilder<'static>>,
        PhantomData::<khora_infra::ui::egui::ui_builder::EguiUiBuilder<'static>>,
    );
    let _ = khora_infra::ui::egui::ui_builder::EguiUiBuilder::new;
    is_ui_builder::<khora_infra::ui::egui::ui_builder::EguiUiBuilder<'static>>();
    let _ = type_name::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>();
    let _ = type_name::<khora_infra::ui::taffy::TaffyLayoutSystem>();
    let _ = type_name::<khora_infra::ui::TaffyLayoutSystem>();
    let _ = type_name::<khora_infra::TaffyLayoutSystem>();
    same_type(
        PhantomData::<khora_infra::ui::taffy::TaffyLayoutSystem>,
        PhantomData::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>,
    );
    same_type(
        PhantomData::<khora_infra::ui::TaffyLayoutSystem>,
        PhantomData::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>,
    );
    same_type(
        PhantomData::<khora_infra::TaffyLayoutSystem>,
        PhantomData::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>,
    );
    let _ = khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem::new;
    is_default::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>();
    is_layout_system::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>();
    is_send::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>();
    is_sync::<khora_infra::ui::taffy::taffy_layout::TaffyLayoutSystem>();
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded; `khora_sdk::khora_infra::…`
// mapped onto the `khora_infra` path the SDK re-exports). The trailing comment
// names the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_infra::audio::cpal::CpalAudioDevice as _; // khora-sdk
    use khora_infra::graphics::WgpuPipelineSystem as _; // khora-sdk
    use khora_infra::graphics::EGUI_WGSL as _; // khora-sdk
    use khora_infra::graphics::TEXT_WGSL as _; // khora-sdk
    use khora_infra::physics::rapier::RapierPhysicsWorld as _; // khora-agents, khora-sdk
    use khora_infra::platform::winit::input::translate_winit_input as _; // khora-sdk
    use khora_infra::platform::winit::WinitWindow as _; // khora-sdk
    use khora_infra::telemetry::memory_monitor::MemoryMonitor as _; // khora-sdk
    use khora_infra::ui::egui::app::run_native as _; // khora-sdk
    use khora_infra::ui::egui::app::WindowConfigInput as _; // khora-sdk
    use khora_infra::ui::egui::app::WindowIconInput as _; // khora-sdk
    use khora_infra::ui::TaffyLayoutSystem as _; // khora-sdk
    use khora_infra::DefaultMixBus as _; // khora-sdk
    use khora_infra::GpuMonitor as _; // khora-sdk
    use khora_infra::SaaTrackingAllocator as _; // khora-sdk
    use khora_infra::StandardTextRenderer as _; // khora-sdk
    use khora_infra::WgpuRenderSystem as _; // khora-sdk
}

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_infra::platform::winit::WinitWindowBuilder::new; // khora-sdk
    let _ = khora_infra::telemetry::memory_monitor::MemoryMonitor::new; // khora-sdk
}
