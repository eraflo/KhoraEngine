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

//! When a requested render-target size is applied.
//!
//! Recreating a target every frame while a dock splitter or a window edge is
//! dragged would allocate and free GPU textures at the frame rate. A
//! [`ResizeDebounce`] applies a new size at once when nothing was applied for
//! [`ResizeDebounce::QUIET`] (a maximize, a first layout); a size that keeps
//! changing faster is held until it is requested
//! [`ResizeDebounce::STABLE_REQUESTS`] more times unchanged, has been pending
//! for [`ResizeDebounce::MAX_PENDING_POLLS`] polls, or `QUIET` has passed
//! since the last apply — so a drag is followed a few times a second, and a
//! released one a few frames later. Time is passed in, so the policy is a pure
//! function of its calls.

use std::time::{Duration, Instant};

/// Holds a requested target size until it is time to apply it.
#[derive(Debug, Default)]
pub(crate) struct ResizeDebounce {
    pending: Option<Pending>,
    /// When `poll` last returned a size; `None` until the first one.
    last_applied: Option<Instant>,
}

/// A size waiting to be applied.
#[derive(Debug)]
struct Pending {
    size: (u32, u32),
    /// Requests of `size` after the first one; a new size restarts it.
    repeats: u32,
    /// Polls since something became pending, whatever size it is now: a drag
    /// that never settles still reaches the target.
    polls: u32,
}

impl ResizeDebounce {
    /// A size is applied once this long has passed since the last apply.
    pub(crate) const QUIET: Duration = Duration::from_millis(120);
    /// A pending size requested this many more times, unchanged, is applied.
    pub(crate) const STABLE_REQUESTS: u32 = 3;
    /// A size pending for this many polls is applied, settled or not.
    pub(crate) const MAX_PENDING_POLLS: u32 = 10;

    /// Nothing pending, nothing applied yet.
    pub(crate) fn new() -> Self {
        Self {
            pending: None,
            last_applied: None,
        }
    }

    /// The size wanted now; called once per frame by whoever owns the shape.
    pub(crate) fn request(&mut self, size: (u32, u32), _now: Instant) {
        match &mut self.pending {
            Some(pending) if pending.size == size => {
                pending.repeats = pending.repeats.saturating_add(1);
            }
            Some(pending) => {
                pending.size = size;
                pending.repeats = 0;
            }
            None => {
                self.pending = Some(Pending {
                    size,
                    repeats: 0,
                    polls: 0,
                });
            }
        }
    }

    /// The size to apply this frame, if any. `current` = the target's size.
    pub(crate) fn poll(&mut self, current: (u32, u32), now: Instant) -> Option<(u32, u32)> {
        let pending = self.pending.as_mut()?;
        if pending.size == current {
            self.pending = None;
            return None;
        }
        pending.polls = pending.polls.saturating_add(1);
        let calm = self
            .last_applied
            .is_none_or(|at| now.saturating_duration_since(at) >= Self::QUIET);
        let settled = calm
            || pending.repeats >= Self::STABLE_REQUESTS
            || pending.polls >= Self::MAX_PENDING_POLLS;
        if !settled {
            return None;
        }
        self.last_applied = Some(now);
        self.pending.take().map(|pending| pending.size)
    }
}

/// Clamps a requested extent to `1..=max` on both sides.
pub(crate) fn clamp_extent(size: (u32, u32), max: u32) -> (u32, u32) {
    let max = max.max(1);
    (size.0.clamp(1, max), size.1.clamp(1, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: (u32, u32) = (1200, 500);
    const B: (u32, u32) = (640, 480);
    const C: (u32, u32) = (1024, 768);
    const CURRENT: (u32, u32) = (800, 600);

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// A debounce that has just applied `A`, and the instant it did. From then
    /// on the target's size is `A`. The apply happens a quiet time after the
    /// request, so it holds under any leading-edge rule.
    fn just_applied_a() -> (ResizeDebounce, Instant) {
        let start = Instant::now();
        let mut debounce = ResizeDebounce::new();
        debounce.request(A, start);
        let applied_at = start + ResizeDebounce::QUIET;
        assert_eq!(
            debounce.poll(CURRENT, applied_at),
            Some(A),
            "the setup applies A"
        );
        (debounce, applied_at)
    }

    #[test]
    fn nothing_requested_applies_nothing() {
        let now = Instant::now();
        let mut debounce = ResizeDebounce::new();
        for frame in 0..(ResizeDebounce::MAX_PENDING_POLLS + 2) {
            assert_eq!(
                debounce.poll(CURRENT, now + ResizeDebounce::QUIET * frame),
                None,
                "poll {frame}"
            );
        }
    }

    #[test]
    fn the_default_debounce_has_nothing_pending() {
        let now = Instant::now();
        let mut debounce = ResizeDebounce::default();
        assert_eq!(debounce.poll(CURRENT, now + ResizeDebounce::QUIET), None);
    }

    #[test]
    fn a_request_for_the_current_size_applies_nothing() {
        let now = Instant::now();
        let mut debounce = ResizeDebounce::new();
        for frame in 0..(ResizeDebounce::MAX_PENDING_POLLS + 2) {
            debounce.request(CURRENT, now);
            assert_eq!(debounce.poll(CURRENT, now), None, "poll {frame}");
        }
        assert_eq!(
            debounce.poll(CURRENT, now + ResizeDebounce::QUIET * 2),
            None,
            "quiet time does not apply the current size either"
        );
    }

    // --- Leading edge -----------------------------------------------------

    /// A first layout shows at once: a debounce that never applied anything
    /// applies its first request at the first poll, with no time passing.
    #[test]
    fn a_fresh_debounce_applies_its_first_request_at_the_first_poll() {
        let now = Instant::now();
        let mut debounce = ResizeDebounce::new();

        debounce.request(A, now);
        assert_eq!(debounce.poll(CURRENT, now), Some(A));
    }

    /// Only the debounce's own applies count: a poll with nothing pending and
    /// a request cancelled because it matched the target are not applies, so
    /// the next request on a debounce that never applied is still immediate.
    #[test]
    fn polls_without_an_apply_leave_the_leading_edge_open() {
        let start = Instant::now();
        let mut debounce = ResizeDebounce::new();

        assert_eq!(debounce.poll(A, start), None, "nothing pending");
        debounce.request(A, start);
        assert_eq!(debounce.poll(A, start), None, "A is the target: cancelled");

        let now = start + ms(1);
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), Some(B), "still never applied");
    }

    /// A maximize after a calm shows at once: a request a full quiet time
    /// after the last apply is applied at its first poll.
    #[test]
    fn a_request_a_quiet_time_after_the_last_apply_is_applied_at_once() {
        let (mut debounce, applied_at) = just_applied_a();

        let now = applied_at + ResizeDebounce::QUIET;
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), Some(B));
    }

    #[test]
    fn a_request_long_after_the_last_apply_is_applied_at_once() {
        let (mut debounce, applied_at) = just_applied_a();

        let now = applied_at + Duration::from_secs(5);
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), Some(B));
    }

    /// Within the quiet time after an apply a new size is held; the first poll
    /// a quiet time after that apply releases it.
    #[test]
    fn a_request_within_the_quiet_time_after_an_apply_is_held_until_it_ends() {
        let (mut debounce, applied_at) = just_applied_a();

        let just_before = applied_at + ResizeDebounce::QUIET - ms(1);
        debounce.request(B, just_before);
        assert_eq!(debounce.poll(A, just_before), None, "1 ms short");
        assert_eq!(
            debounce.poll(A, applied_at + ResizeDebounce::QUIET),
            Some(B),
            "a quiet time after the apply"
        );
    }

    /// The quiet time counts from the most recent apply: right after a
    /// leading-edge apply, the next size is held again.
    #[test]
    fn the_quiet_time_counts_from_the_most_recent_apply() {
        let (mut debounce, applied_a) = just_applied_a();

        let applied_b = applied_a + ResizeDebounce::QUIET;
        debounce.request(B, applied_b);
        assert_eq!(debounce.poll(A, applied_b), Some(B), "B, leading edge");

        let now = applied_b + ms(50);
        debounce.request(C, now);
        assert_eq!(debounce.poll(B, now), None, "C, 50 ms after B");
        assert_eq!(
            debounce.poll(B, applied_b + ResizeDebounce::QUIET),
            Some(C),
            "C, a quiet time after B"
        );
    }

    /// A splitter dragged at 60 Hz (a new size every 16 ms): the first size
    /// shows at once, then one size per quiet time — frame 8 (128 ms after
    /// the first apply) and frame 16 (128 ms after that one), each the size
    /// requested that frame. Frame 7 (112 ms) and frame 15 (112 ms after the
    /// apply at frame 8) are still inside the quiet time; no size repeats, and
    /// no pending size reaches ten polls.
    #[test]
    fn a_drag_at_sixty_hertz_applies_its_first_size_then_one_per_quiet_time() {
        let start = Instant::now();
        let mut debounce = ResizeDebounce::new();
        let size_of_frame = |frame: u32| (1000 + frame, 500 + frame);
        let at_frame = |frame: u32| start + ms(16 * u64::from(frame));

        debounce.request(size_of_frame(0), at_frame(0));
        assert_eq!(
            debounce.poll(CURRENT, at_frame(0)),
            Some(size_of_frame(0)),
            "frame 0: first size, at once"
        );
        let mut current = size_of_frame(0);
        for frame in 1..=16 {
            debounce.request(size_of_frame(frame), at_frame(frame));
            let applied = debounce.poll(current, at_frame(frame));
            let expected = matches!(frame, 8 | 16).then(|| size_of_frame(frame));
            assert_eq!(applied, expected, "frame {frame}");
            if let Some(size) = applied {
                current = size;
            }
        }
    }

    /// A drag faster than the quiet time can fill (a new size every 5 ms)
    /// still reaches the target every tenth poll: frame 10 (50 ms after the
    /// apply), then frame 20 (ten polls after that).
    #[test]
    fn a_fast_drag_is_applied_every_tenth_poll() {
        let (mut debounce, applied_at) = just_applied_a();
        let size_of_frame = |frame: u32| (1000 + frame, 500 + frame);
        let at_frame = |frame: u32| applied_at + ms(5 * u64::from(frame));

        let mut current = A;
        for frame in 1..=20 {
            debounce.request(size_of_frame(frame), at_frame(frame));
            let applied = debounce.poll(current, at_frame(frame));
            let expected = matches!(frame, 10 | 20).then(|| size_of_frame(frame));
            assert_eq!(applied, expected, "frame {frame}");
            if let Some(size) = applied {
                current = size;
            }
        }
    }

    // --- Held sizes (within the quiet time after an apply) ---------------

    /// The first request of a size is not a stable one; each identical
    /// request after it is. The fourth request in a row — three repeats —
    /// applies it, well within the quiet time after the last apply.
    #[test]
    fn a_size_requested_three_more_times_unchanged_is_applied() {
        let (mut debounce, applied_at) = just_applied_a();
        let now = applied_at + ms(1);

        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), None, "first request");
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), None, "one repeat");
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), None, "two repeats");
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), Some(B), "three repeats");
    }

    /// A drag that never settles still reaches the target: a size changing
    /// every frame, with no time passing, is applied by the tenth poll — the
    /// latest size, not the first.
    #[test]
    fn sizes_changing_every_frame_are_applied_by_the_tenth_poll() {
        let (mut debounce, applied_at) = just_applied_a();
        let now = applied_at + ms(1);
        let size_of_frame = |frame: u32| (1000 + frame, 500 + frame);

        for frame in 1..ResizeDebounce::MAX_PENDING_POLLS {
            debounce.request(size_of_frame(frame), now);
            assert_eq!(debounce.poll(A, now), None, "poll {frame}");
        }
        let last = size_of_frame(ResizeDebounce::MAX_PENDING_POLLS);
        debounce.request(last, now);
        assert_eq!(debounce.poll(A, now), Some(last), "tenth poll");
    }

    /// One request, never repeated, held because an apply just happened, is
    /// applied by the first poll a quiet time after that apply — not a quiet
    /// time after the request.
    #[test]
    fn a_single_held_request_is_applied_a_quiet_time_after_the_last_apply() {
        let (mut debounce, applied_at) = just_applied_a();

        let requested = applied_at + ms(10);
        debounce.request(B, requested);
        assert_eq!(debounce.poll(A, requested), None, "at once");
        assert_eq!(
            debounce.poll(A, applied_at + ms(119)),
            None,
            "before the quiet time"
        );
        assert_eq!(
            debounce.poll(A, applied_at + ResizeDebounce::QUIET),
            Some(B),
            "a quiet time after the apply"
        );
    }

    /// A size that changes while held: the latest one is what the end of the
    /// quiet time applies.
    #[test]
    fn a_size_changed_while_held_applies_the_latest_size() {
        let (mut debounce, applied_at) = just_applied_a();

        debounce.request(B, applied_at + ms(10));
        assert_eq!(debounce.poll(A, applied_at + ms(10)), None, "B");
        debounce.request(C, applied_at + ms(100));
        assert_eq!(debounce.poll(A, applied_at + ms(100)), None, "C");
        assert_eq!(
            debounce.poll(A, applied_at + ResizeDebounce::QUIET),
            Some(C),
            "a quiet time after the apply"
        );
    }

    /// A new size restarts the stable count: B B B C is the first request of
    /// C, so C needs three repeats of its own.
    #[test]
    fn a_new_size_restarts_the_stable_count() {
        let (mut debounce, applied_at) = just_applied_a();
        let now = applied_at + ms(1);

        for _ in 0..3 {
            debounce.request(B, now);
            assert_eq!(debounce.poll(A, now), None);
        }
        debounce.request(C, now);
        assert_eq!(debounce.poll(A, now), None, "C, first request");
        debounce.request(C, now);
        assert_eq!(debounce.poll(A, now), None, "C, one repeat");
        debounce.request(C, now);
        assert_eq!(debounce.poll(A, now), None, "C, two repeats");
        debounce.request(C, now);
        assert_eq!(debounce.poll(A, now), Some(C), "C, three repeats");
    }

    #[test]
    fn an_applied_size_is_applied_once() {
        let start = Instant::now();
        let mut debounce = ResizeDebounce::new();

        debounce.request(A, start);
        assert_eq!(
            debounce.poll(CURRENT, start + ResizeDebounce::QUIET),
            Some(A)
        );
        for frame in 1..=(ResizeDebounce::MAX_PENDING_POLLS + 1) {
            assert_eq!(
                debounce.poll(CURRENT, start + ResizeDebounce::QUIET * (frame + 1)),
                None,
                "poll {frame} after it was applied"
            );
        }
    }

    /// The next size after a held one was applied is debounced afresh: it
    /// waits for its own repeats, none carried over from the size before it.
    #[test]
    fn a_size_requested_after_one_was_applied_waits_again() {
        let (mut debounce, applied_at) = just_applied_a();
        let now = applied_at + ms(1);

        for _ in 0..3 {
            debounce.request(B, now);
            assert_eq!(debounce.poll(A, now), None);
        }
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), Some(B), "B, three repeats");

        let next = now + ms(1);
        debounce.request(C, next);
        assert_eq!(debounce.poll(B, next), None, "C, first request");
        debounce.request(C, next);
        assert_eq!(debounce.poll(B, next), None, "C, one repeat");
        debounce.request(C, next);
        assert_eq!(debounce.poll(B, next), None, "C, two repeats");
        debounce.request(C, next);
        assert_eq!(debounce.poll(B, next), Some(C), "C, three repeats");
    }

    /// Dragging away and back before the debounce fires leaves the target as
    /// it is: the request for the current size cancels the pending one, and
    /// nothing is applied later either.
    #[test]
    fn a_request_back_to_the_current_size_cancels_the_pending_one() {
        let (mut debounce, applied_at) = just_applied_a();
        let now = applied_at + ms(1);

        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), None, "B held");
        debounce.request(A, now);
        assert_eq!(debounce.poll(A, now), None, "back to the current size");
        for frame in 1..=(ResizeDebounce::MAX_PENDING_POLLS + 1) {
            assert_eq!(
                debounce.poll(A, now + ResizeDebounce::QUIET * frame),
                None,
                "poll {frame} after the cancel"
            );
        }
    }

    // --- The target changed by something other than the debounce ---------

    /// The surface can be reconfigured outside the debounce (a lost or
    /// outdated swapchain is reconfigured to the wanted size at acquire).
    /// A held size the target reached that way is dropped, not applied a
    /// second time once the quiet time ends.
    #[test]
    fn a_held_size_the_target_reached_another_way_is_not_applied_again() {
        let (mut debounce, applied_at) = just_applied_a();

        let now = applied_at + ms(10);
        debounce.request(B, now);
        assert_eq!(debounce.poll(A, now), None, "B held");

        // Reconfigured to B by someone else; B is still the size wanted.
        for frame in 1..=(ResizeDebounce::MAX_PENDING_POLLS + 1) {
            let at = now + ResizeDebounce::QUIET * frame;
            debounce.request(B, at);
            assert_eq!(
                debounce.poll(B, at),
                None,
                "poll {frame} once B is the target"
            );
        }
    }

    /// The other way round: the debounce applied A, then the target was set
    /// to some other size behind its back while A is still wanted. The
    /// debounce brings it back to A — held within the quiet time after its
    /// apply, then applied.
    #[test]
    fn a_target_moved_away_from_the_wanted_size_is_brought_back() {
        let (mut debounce, applied_at) = just_applied_a();

        let now = applied_at + ms(10);
        debounce.request(A, now);
        assert_eq!(debounce.poll(C, now), None, "within the quiet time");
        let later = applied_at + ResizeDebounce::QUIET;
        debounce.request(A, later);
        assert_eq!(
            debounce.poll(C, later),
            Some(A),
            "a quiet time after the apply"
        );
    }

    #[test]
    fn clamp_extent_raises_a_zero_side_to_one() {
        assert_eq!(clamp_extent((0, 5), 8192), (1, 5));
        assert_eq!(clamp_extent((5, 0), 8192), (5, 1));
        assert_eq!(clamp_extent((0, 0), 8192), (1, 1));
    }

    #[test]
    fn clamp_extent_lowers_a_side_over_the_limit_to_the_limit() {
        assert_eq!(clamp_extent((9000, 10), 8192), (8192, 10));
        assert_eq!(clamp_extent((10, 9000), 8192), (10, 8192));
    }

    #[test]
    fn clamp_extent_keeps_a_size_within_the_limits() {
        assert_eq!(clamp_extent((1200, 500), 8192), (1200, 500));
        assert_eq!(clamp_extent((1, 8192), 8192), (1, 8192));
    }
}
