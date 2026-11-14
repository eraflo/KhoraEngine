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

//! `WinitWindowProvider` — the window provider backed by winit.

use std::any::Any;
use std::sync::Arc;

use khora_core::platform::KhoraWindow;
use khora_infra::platform::winit::WinitWindow;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;

use crate::traits::WindowProvider;
use crate::{InputEvent, WindowConfig};

/// A window provider backed by winit.
pub struct WinitWindowProvider {
    window: WinitWindow,
}

impl WindowProvider for WinitWindowProvider {
    /// Creates the window hidden; it is shown once its first frame is painted.
    ///
    /// # Panics
    ///
    /// When `native_loop` is not winit's `ActiveEventLoop`, or the platform
    /// refuses to open the window.
    fn create(native_loop: &dyn Any, config: &WindowConfig) -> Self
    where
        Self: Sized,
    {
        let event_loop = native_loop
            .downcast_ref::<ActiveEventLoop>()
            .expect("WindowProvider::create called with wrong native_loop type");

        // Hidden until its first frame is painted: the engine boots behind
        // it, and the operating system would show an empty window meanwhile.
        let mut builder = khora_infra::platform::winit::WinitWindowBuilder::new()
            .with_title(&config.title)
            .with_dimensions(config.width, config.height)
            .with_visible(false);

        if let Some(icon) = &config.icon {
            builder = builder.with_icon_rgba(icon.rgba.clone(), icon.width, icon.height);
        }

        let window = builder.build(event_loop).expect("Failed to create window");
        Self { window }
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }

    fn inner_size(&self) -> (u32, u32) {
        self.window.inner_size()
    }

    fn scale_factor(&self) -> f64 {
        self.window.scale_factor()
    }

    fn as_khora_window(&self) -> &dyn KhoraWindow {
        &self.window
    }

    fn clone_raw_window_arc(&self) -> Arc<dyn Any + Send + Sync> {
        self.window.clone_winit_arc()
    }

    fn translate_event(&self, raw_event: &dyn Any) -> Option<InputEvent> {
        if let Some(winit_event) = raw_event.downcast_ref::<WindowEvent>() {
            khora_infra::platform::winit::input::translate_winit_input(winit_event)
        } else {
            None
        }
    }
}
