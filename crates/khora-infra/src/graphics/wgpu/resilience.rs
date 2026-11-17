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

//! The surface-acquire and device-loss decisions, free of wgpu handles so they test headless.
//!
//! See the book: concepts/rendering.md, "Device-loss resilience".

pub(crate) use super::gpu_health::GpuHealth;

/// The status of a swapchain acquire attempt, decoupled from the wgpu handle.
///
/// Mirrors the variants of [`wgpu::CurrentSurfaceTexture`], so the policy of
/// [`classify_acquire`] runs without a GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceAcquireStatus {
    /// A usable texture was acquired (`Success` or `Suboptimal`).
    Usable,
    /// The surface is lost or outdated and must be reconfigured.
    LostOrOutdated,
    /// A transient timeout — skip this frame and retry next frame.
    Timeout,
    /// The window is occluded (minimized / hidden) — skip this frame.
    Occluded,
    /// A validation error was raised during acquisition.
    Validation,
    /// A future wgpu variant not known at compile time.
    Unknown,
}

/// What the render system should do after an acquire attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AcquireAction {
    /// Use the acquired texture and proceed with the frame.
    Proceed,
    /// Reconfigure the surface with the current size, then retry the acquire
    /// within the same frame (the swapchain is recoverable in place).
    ReconfigureAndRetry,
    /// Skip this frame and try again next frame; at most a warning is logged,
    /// no error reaches the host.
    SkipFrame,
    /// Skip the frame and report a non-fatal error; the host keeps running.
    NonFatalError,
}

/// Classifies a swapchain acquire status into the action the render system takes.
///
/// `has_valid_size` is `false` for a zero-size (minimized) window: a lost
/// surface then skips the frame, as there is no size to reconfigure to.
pub(crate) fn classify_acquire(
    status: SurfaceAcquireStatus,
    has_valid_size: bool,
) -> AcquireAction {
    match status {
        SurfaceAcquireStatus::Usable => AcquireAction::Proceed,
        SurfaceAcquireStatus::LostOrOutdated => {
            if has_valid_size {
                AcquireAction::ReconfigureAndRetry
            } else {
                AcquireAction::SkipFrame
            }
        }
        // Transient: a frame was simply not ready in time.
        SurfaceAcquireStatus::Timeout => AcquireAction::SkipFrame,
        // Minimized / hidden: presenting is pointless until it is shown again.
        SurfaceAcquireStatus::Occluded => AcquireAction::SkipFrame,
        // Validation errors are caught by the device error handler; surface a
        // non-fatal error so the caller logs it, but keep the loop alive.
        SurfaceAcquireStatus::Validation => AcquireAction::NonFatalError,
        // Forward-compat: an unfamiliar variant is treated as a skippable,
        // non-fatal hiccup rather than a panic-by-omission.
        SurfaceAcquireStatus::Unknown => AcquireAction::NonFatalError,
    }
}

/// Returns whether a surface of this size can be rendered: a zero axis (minimized window) cannot.
///
/// The caller skips such a frame without logging an error.
pub(crate) fn surface_is_renderable(width: u32, height: u32) -> bool {
    width > 0 && height > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usable_status_proceeds() {
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::Usable, true),
            AcquireAction::Proceed
        );
        // Size is irrelevant once a texture is in hand.
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::Usable, false),
            AcquireAction::Proceed
        );
    }

    #[test]
    fn lost_or_outdated_with_size_reconfigures() {
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::LostOrOutdated, true),
            AcquireAction::ReconfigureAndRetry
        );
    }

    #[test]
    fn lost_or_outdated_zero_size_skips_without_error() {
        // A minimized window must not spam a hard error every frame.
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::LostOrOutdated, false),
            AcquireAction::SkipFrame
        );
    }

    #[test]
    fn timeout_and_occluded_skip_frame() {
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::Timeout, true),
            AcquireAction::SkipFrame
        );
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::Occluded, true),
            AcquireAction::SkipFrame
        );
    }

    #[test]
    fn validation_and_unknown_are_non_fatal() {
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::Validation, true),
            AcquireAction::NonFatalError
        );
        assert_eq!(
            classify_acquire(SurfaceAcquireStatus::Unknown, true),
            AcquireAction::NonFatalError
        );
    }

    #[test]
    fn surface_renderable_requires_nonzero_dimensions() {
        assert!(surface_is_renderable(1280, 720));
        assert!(!surface_is_renderable(0, 720));
        assert!(!surface_is_renderable(1280, 0));
        assert!(!surface_is_renderable(0, 0));
    }
}
