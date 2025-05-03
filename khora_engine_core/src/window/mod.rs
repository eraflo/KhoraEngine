
use std::sync::Arc;
use winit::{
    dpi::PhysicalSize,
    error::OsError,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes},
    window::WindowId,
};

#[cfg(feature = "raw-window-handle")]
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};


/// A wrapper around a winit window, providing controlled access and engine-specific utilities.
#[derive(Debug, Clone)]
pub struct KhoraWindow {
    inner: Arc<Window>,
}

impl KhoraWindow {

    /// Creates a new KhoraWindow wrapper by building and wrapping a winit window.
    /// ## Arguments
    /// * `event_loop` - The active winit event loop target needed to create the window.
    /// ## Returns
    /// A `Result` containing the new `KhoraWindow` or a `winit::error::OsError` on failure.
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, OsError> {

        log::info!("Creating application window via KhoraWindow wrapper...");
        
        let window_attributes = Window::default_attributes()
            .with_title("Khora Engine")
            .with_inner_size(winit::dpi::LogicalSize::new(1024, 768))
            .with_visible(true); // Ensure window is initially visible

        // Create the window using the event loop and attributes
        let window = event_loop.create_window(window_attributes)?;


        log::info!("Window created successfully (id: {:?}).", window.id());
        Ok(Self {
            inner: Arc::new(window),
        })
    }


    /// Returns the unique identifier of the underlying window.
    /// ## Returns
    /// The `WindowId` of the wrapped window.
    pub fn id(&self) -> WindowId {
        self.inner.id()
    }

    /// Requests that a redraw event be emitted for this window.
    pub fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    /// Returns the physical size of the window's client area.
    /// ## Returns
    /// The `PhysicalSize<u32>` representing the inner size of the window.
    pub fn inner_size(&self) -> PhysicalSize<u32> {
        self.inner.inner_size()
    }

    /// Returns the display scale factor associated with this window.
    /// ## Returns
    /// The scale factor as a `f64`.
    pub fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    /// Returns the window handle associated with this window.
    /// ## Returns
    /// The window handle as a `RawWindowHandle`.
    #[cfg(feature = "raw-window-handle")]
    pub fn raw_window_handle(&self,) -> Result<raw_window_handle::RawWindowHandle, raw_window_handle::HandleError> {
        self.inner.window_handle().map(|h| h.as_raw())
    }

    /// Returns the display handle associated with this window.
    /// ## Returns
    /// The display handle as a `RawDisplayHandle`.
    #[cfg(feature = "raw-window-handle")]
    pub fn raw_display_handle(&self,) -> Result<raw_window_handle::RawDisplayHandle, raw_window_handle::HandleError> {
        self.inner.display_handle().map(|h| h.as_raw())
    }

    /// Returns a reference to the underlying winit window.
    /// ## Returns
    /// A reference to the `Window` wrapped in an `Arc`.
    pub(crate) fn winit_window_arc(&self) -> &Arc<Window> { &self.inner }


}