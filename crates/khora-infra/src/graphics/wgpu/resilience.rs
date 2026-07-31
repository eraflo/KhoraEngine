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

//! Pure decision logic for GPU surface-acquisition and device-loss resilience.
//!
//! The functions here are intentionally free of any live wgpu handle so they
//! can be unit-tested headless in CI. The render system maps a real
//! `wgpu::CurrentSurfaceTexture` (and device-loss flag) onto these classifiers
//! and then performs the side effects (reconfigure / skip / error) that each
//! decision prescribes.

use std::sync::atomic::{AtomicBool, Ordering};

/// Shared, lock-free health flags for a GPU device.
///
/// Populated by the wgpu device error callbacks (uncaptured-error and
/// device-lost) registered at device creation, and read by the render system
/// once per frame to decide whether to keep submitting work. The flags are
/// sticky: once raised they stay raised, because the device cannot recover
/// these conditions in place.
#[derive(Debug, Default)]
pub(crate) struct GpuHealth {
    /// Set when the device was reported lost (driver crash/reset/destroy) or an
    /// internal device error was observed.
    device_lost: AtomicBool,
    /// Set when the device/surface reported an out-of-memory condition.
    out_of_memory: AtomicBool,
}

impl GpuHealth {
    /// Marks the device as lost. Idempotent and callable from the wgpu
    /// callback thread.
    pub(crate) fn mark_device_lost(&self) {
        self.device_lost.store(true, Ordering::SeqCst);
    }

    /// Marks the device as out-of-memory. Idempotent and callable from the
    /// wgpu callback thread.
    pub(crate) fn mark_out_of_memory(&self) {
        self.out_of_memory.store(true, Ordering::SeqCst);
    }

    /// Whether the device has been reported lost.
    pub(crate) fn is_device_lost(&self) -> bool {
        self.device_lost.load(Ordering::SeqCst)
    }

    /// Whether the device has reported an out-of-memory condition.
    pub(crate) fn is_out_of_memory(&self) -> bool {
        self.out_of_memory.load(Ordering::SeqCst)
    }
}

/// The status of a swapchain acquire attempt, decoupled from the wgpu handle.
///
/// Mirrors the discriminants of [`wgpu::CurrentSurfaceTexture`] so the
/// classification policy can be exercised without a GPU. The render system
/// maps the real enum onto this before consulting [`classify_acquire`].
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
    /// Skip this frame silently-ish (a `log::warn`/`debug` is fine) and try
    /// again next frame. No error is surfaced to the host.
    SkipFrame,
    /// A non-fatal acquisition error: skip the frame and report it upward, but
    /// the host may keep running.
    NonFatalError,
}

/// Classify a swapchain acquire status into the action the render system takes.
///
/// `has_valid_size` is `false` when the stored surface dimensions are zero
/// (a minimized / zero-size window). In that case a lost/outdated surface
/// cannot be reconfigured, so we skip the frame instead of spamming a hard
/// error every frame until a real resize event arrives.
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
                // Zero-size window: nothing to reconfigure to. Wait for a
                // resize event rather than erroring every frame.
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

/// Whether a render frame should be attempted at all given the current surface
/// dimensions. A zero in either axis (minimized window) means there is no
/// renderable surface — the caller skips the frame without logging an error.
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

    #[test]
    fn gpu_health_starts_clean() {
        let health = GpuHealth::default();
        assert!(!health.is_device_lost());
        assert!(!health.is_out_of_memory());
    }

    #[test]
    fn gpu_health_device_lost_is_sticky() {
        let health = GpuHealth::default();
        health.mark_device_lost();
        assert!(health.is_device_lost());
        // Sticky: a second read still reports lost.
        assert!(health.is_device_lost());
    }

    #[test]
    fn gpu_health_out_of_memory_independent_of_lost() {
        let health = GpuHealth::default();
        health.mark_out_of_memory();
        assert!(health.is_out_of_memory());
        assert!(!health.is_device_lost());
    }
}
