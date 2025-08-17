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

use khora_core::platform::window::{KhoraWindow, KhoraWindowHandle};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
}; // Importez les types nécessaires
use std::sync::Arc;
use winit::{dpi::LogicalSize, error::OsError, event_loop::ActiveEventLoop, window::Window};

#[derive(Debug, Clone)]
pub struct WinitWindow {
    inner: Arc<Window>,
}

pub struct WinitWindowBuilder {
    title: String,
    width: u32,
    height: u32,
}

impl WinitWindowBuilder {
    pub fn new() -> Self {
        Self {
            title: "Khora Engine".to_string(),
            width: 1024,
            height: 768,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_dimensions(mut self, width: u32, height: u32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn build(self, event_loop: &ActiveEventLoop) -> Result<WinitWindow, OsError> {
        log::info!(
            "Building window with title: '{}' and size: {}x{}",
            self.title,
            self.width,
            self.height
        );

        let window_attributes = Window::default_attributes()
            .with_title(self.title)
            .with_inner_size(LogicalSize::new(self.width, self.height))
            .with_visible(true);

        let window = event_loop.create_window(window_attributes)?;

        log::info!("Winit window created successfully (id: {:?}).", window.id());
        Ok(WinitWindow {
            inner: Arc::new(window),
        })
    }
}

impl Default for WinitWindowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl HasWindowHandle for WinitWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.inner.window_handle()
    }
}

impl HasDisplayHandle for WinitWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.inner.display_handle()
    }
}

impl KhoraWindow for WinitWindow {
    fn inner_size(&self) -> (u32, u32) {
        let size = self.inner.inner_size();
        (size.width, size.height)
    }

    fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    fn clone_handle_arc(&self) -> KhoraWindowHandle {
        self.inner.clone()
    }

    fn id(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.inner.id().hash(&mut hasher);
        hasher.finish()
    }
}
