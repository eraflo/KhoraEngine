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

//! The frame's clock: how long a frame kept the host busy, the waits for the
//! display excluded.

use std::time::{Duration, Instant};

/// Measures one frame's busy time on the host: from [`open`](Self::open) to
/// [`close`](Self::close), less what ran inside [`wait`](Self::wait).
#[derive(Debug, Default)]
pub(crate) struct FrameClock {
    /// When the frame opened; `None` between frames.
    opened: Option<Instant>,
    /// What the open frame spent waiting so far.
    waited: Duration,
}

impl FrameClock {
    /// Starts a frame; a second `open` restarts it.
    pub(crate) fn open(&mut self) {
        self.opened = Some(Instant::now());
        self.waited = Duration::ZERO;
    }

    /// Runs `f`, its duration excluded from the frame. No effect on the
    /// result when the clock is not open.
    pub(crate) fn wait<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let result = f();
        if self.opened.is_some() {
            self.waited += started.elapsed();
        }
        result
    }

    /// The busy time since [`open`](Self::open), waits excluded, and closes
    /// the clock; `None` when it was not open.
    pub(crate) fn close(&mut self) -> Option<Duration> {
        let opened = self.opened.take()?;
        Some(opened.elapsed().saturating_sub(self.waited))
    }
}

#[cfg(test)]
mod tests {
    use super::FrameClock;
    use std::thread;
    use std::time::{Duration, Instant};

    /// **A frame's busy time excludes what it waited for.** A frame works
    /// 20 ms and waits 60 ms for the display: the clock reports at least the
    /// 20 ms of work and none of the 60 ms waited.
    #[test]
    fn busy_time_excludes_what_the_frame_waited_for() {
        let mut clock = FrameClock::default();
        let outside = Instant::now();

        clock.open();
        thread::sleep(Duration::from_millis(20));
        clock.wait(|| thread::sleep(Duration::from_millis(60)));
        let busy = clock.close().expect("an open clock measures its frame");
        let total = outside.elapsed();

        assert!(
            busy >= Duration::from_millis(20),
            "the work counts: {busy:?}"
        );
        assert!(
            busy + Duration::from_millis(60) <= total,
            "the wait does not count: {busy:?} busy in {total:?}"
        );
    }

    /// **A clock never opened measures nothing.**
    #[test]
    fn closing_a_clock_never_opened_measures_nothing() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.close(), None);
    }

    /// **Closing twice measures once.** `close` closes the clock: the second
    /// `close` has no frame to measure.
    #[test]
    fn closing_twice_measures_once() {
        let mut clock = FrameClock::default();
        clock.open();

        assert!(
            clock.close().is_some(),
            "the first close measures the frame"
        );
        assert_eq!(clock.close(), None, "the second has none");
    }

    /// **A wait returns what it ran, inside a frame or not.**
    #[test]
    fn a_wait_returns_what_it_ran_inside_a_frame_or_not() {
        let mut clock = FrameClock::default();
        assert_eq!(clock.wait(|| 42), 42, "outside a frame");

        clock.open();
        assert_eq!(clock.wait(|| "presented"), "presented", "inside a frame");
        assert!(clock.close().is_some());
    }

    /// **A wait outside a frame is not taken from the next one.** 30 ms
    /// waited before the frame opened, then 10 ms of work: the frame was busy
    /// at least those 10 ms.
    #[test]
    fn a_wait_outside_a_frame_is_not_taken_from_the_next() {
        let mut clock = FrameClock::default();
        clock.wait(|| thread::sleep(Duration::from_millis(30)));

        clock.open();
        thread::sleep(Duration::from_millis(10));
        let busy = clock.close().expect("an open clock measures its frame");

        assert!(busy >= Duration::from_millis(10), "{busy:?}");
    }

    /// **A second `open` restarts the frame.** 40 ms of work before the
    /// second `open` belong to no frame: the frame closed right after it was
    /// reopened is busy less than that.
    #[test]
    fn a_second_open_restarts_the_frame() {
        let mut clock = FrameClock::default();
        clock.open();
        thread::sleep(Duration::from_millis(40));

        let reopened = Instant::now();
        clock.open();
        let busy = clock.close().expect("an open clock measures its frame");

        assert!(
            busy <= reopened.elapsed(),
            "only the time since the second open counts: {busy:?}"
        );
    }

    /// **The waits of one frame add up.** Two waits of 30 ms each, around
    /// 10 ms of work, leave none of the 60 ms in the busy time.
    #[test]
    fn the_waits_of_one_frame_add_up() {
        let mut clock = FrameClock::default();
        let outside = Instant::now();

        clock.open();
        clock.wait(|| thread::sleep(Duration::from_millis(30)));
        thread::sleep(Duration::from_millis(10));
        clock.wait(|| thread::sleep(Duration::from_millis(30)));
        let busy = clock.close().expect("an open clock measures its frame");
        let total = outside.elapsed();

        assert!(
            busy >= Duration::from_millis(10),
            "the work counts: {busy:?}"
        );
        assert!(
            busy + Duration::from_millis(60) <= total,
            "neither wait counts: {busy:?} busy in {total:?}"
        );
    }
}
