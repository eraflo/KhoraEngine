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

use super::*;

/// Appends, like an input event or a contact.
#[derive(Debug, Clone, PartialEq)]
struct Happening(u32);
impl Supersedes for Happening {}

/// Replaces by name, like a recompiled module.
#[derive(Debug, Clone, PartialEq)]
struct Recompiled {
    module: &'static str,
    version: u32,
}
impl Supersedes for Recompiled {
    const COALESCES: bool = true;

    fn supersedes(&self, earlier: &Self) -> bool {
        self.module == earlier.module
    }
}

fn channel<T: Supersedes>(capacity: usize) -> Channel<T> {
    Channel::bounded(capacity, WhenFull::DropOldest)
}

// ─── Reading ────────────────────────────────────────────────────────────────

#[test]
fn a_reader_sees_what_was_sent_and_then_nothing() {
    let events = channel(8);
    let mut cursor = Cursor::default();
    events.send(Happening(1));
    events.send(Happening(2));

    assert_eq!(events.read(&mut cursor), vec![Happening(1), Happening(2)]);
    assert!(
        events.read(&mut cursor).is_empty(),
        "the same cursor does not see them twice"
    );
}

/// **The reason the cursor is the reader's.** Two agents reading the same
/// channel answer disjoint questions; neither may starve the other, and neither
/// may serialise against the other.
#[test]
fn two_readers_each_see_everything() {
    let events = channel(8);
    let (mut one, mut other) = (Cursor::default(), Cursor::default());
    events.send(Happening(1));
    events.send(Happening(2));

    assert_eq!(events.read(&mut one).len(), 2);
    assert_eq!(
        events.read(&mut other).len(),
        2,
        "the first reader took nothing from the second"
    );
}

/// A reader that subscribes after the fact still hears what is retained — a
/// system registered a frame late is not a system that missed the frame.
#[test]
fn a_reader_that_arrives_late_hears_what_is_held() {
    let events = channel(8);
    events.send(Happening(1));
    events.send(Happening(2));

    let mut latecomer = Cursor::default();
    assert_eq!(events.read(&mut latecomer).len(), 2);
}

#[test]
fn reading_is_interleaved_with_sending() {
    let events = channel(8);
    let mut cursor = Cursor::default();

    events.send(Happening(1));
    assert_eq!(events.read(&mut cursor), vec![Happening(1)]);
    events.send(Happening(2));
    assert_eq!(events.read(&mut cursor), vec![Happening(2)]);
}

// ─── Being full ─────────────────────────────────────────────────────────────

/// **A queue that discards in silence is a failure nobody can see.**
#[test]
fn dropping_the_oldest_is_counted() {
    let events: Channel<Happening> = Channel::bounded(2, WhenFull::DropOldest);
    events.send(Happening(1));
    events.send(Happening(2));
    events.send(Happening(3));

    let mut cursor = Cursor::default();
    assert_eq!(events.read(&mut cursor), vec![Happening(2), Happening(3)]);
    assert_eq!(events.dropped(), 1);
}

#[test]
fn rejecting_keeps_what_is_already_queued() {
    let events: Channel<Happening> = Channel::bounded(2, WhenFull::Reject);
    events.send(Happening(1));
    events.send(Happening(2));
    events.send(Happening(3));

    let mut cursor = Cursor::default();
    assert_eq!(
        events.read(&mut cursor),
        vec![Happening(1), Happening(2)],
        "the newcomer was refused, not the incumbent"
    );
    assert_eq!(events.dropped(), 1);
}

/// A cursor the drops have run past resumes at the oldest item still held. It
/// loses history — that is what dropping means — but never its place, which
/// would make it read the whole channel again or nothing ever again.
#[test]
fn a_cursor_left_behind_by_drops_resumes_at_the_oldest_held() {
    let events: Channel<Happening> = Channel::bounded(2, WhenFull::DropOldest);
    let mut cursor = Cursor::default();
    events.send(Happening(1));
    events.read(&mut cursor);

    events.send(Happening(2));
    events.send(Happening(3));
    events.send(Happening(4));

    assert_eq!(events.read(&mut cursor), vec![Happening(3), Happening(4)]);
    assert!(events.read(&mut cursor).is_empty());
}

// ─── Coalescing ─────────────────────────────────────────────────────────────

/// **Two saves of the same file between frames are one reload.** Applying the
/// older after the newer would leave the game running code the author replaced.
#[test]
fn an_item_that_supersedes_replaces_in_place() {
    let reloads = channel(8);
    reloads.send(Recompiled {
        module: "ai/guard.erg",
        version: 1,
    });
    reloads.send(Recompiled {
        module: "loot/chest.erg",
        version: 1,
    });
    reloads.send(Recompiled {
        module: "ai/guard.erg",
        version: 2,
    });

    let mut cursor = Cursor::default();
    let read = reloads.read(&mut cursor);
    assert_eq!(read.len(), 2, "{read:?}");
    assert_eq!(
        read[0].version, 2,
        "the later save won, where the first sat"
    );
    assert_eq!(read[0].module, "ai/guard.erg", "and kept its position");
}

/// The default is to append. Two collisions in one frame are two collisions,
/// and a producer that meant to send one should send one.
#[test]
fn items_that_supersede_nothing_all_arrive() {
    let events = channel(8);
    events.send(Happening(7));
    events.send(Happening(7));

    let mut cursor = Cursor::default();
    assert_eq!(events.read(&mut cursor).len(), 2);
}

/// Coalescing never drops: it replaces an unread item, so the channel cannot
/// fill up on repeats of one subject.
#[test]
fn coalescing_does_not_count_as_a_drop() {
    let reloads: Channel<Recompiled> = Channel::bounded(1, WhenFull::Reject);
    reloads.send(Recompiled {
        module: "ai/guard.erg",
        version: 1,
    });
    reloads.send(Recompiled {
        module: "ai/guard.erg",
        version: 2,
    });

    assert_eq!(reloads.dropped(), 0);
    assert_eq!(reloads.len(), 1);
}

// ─── Draining ───────────────────────────────────────────────────────────────

#[test]
fn draining_empties_the_channel() {
    let events = channel(8);
    events.send(Happening(1));
    events.send(Happening(2));

    assert_eq!(events.drain(), vec![Happening(1), Happening(2)]);
    assert!(events.is_empty());
}

/// A drain moves the stream on, so a subscriber does not then read what the
/// drain already took — the two disciplines share one position line.
#[test]
fn a_reader_does_not_see_what_a_drain_took() {
    let events = channel(8);
    let mut cursor = Cursor::default();
    events.send(Happening(1));

    events.drain();

    assert!(events.read(&mut cursor).is_empty());
}

// ─── The handle ─────────────────────────────────────────────────────────────

/// Two handles are one channel — that is what the `Arc` is for, and what lets a
/// producer hold one while the runtime holds another.
#[test]
fn a_clone_shares_the_stream() {
    let producer = channel(8);
    let consumer = producer.clone();
    producer.send(Happening(7));

    let mut cursor = Cursor::default();
    assert_eq!(consumer.read(&mut cursor), vec![Happening(7)]);
}

#[test]
#[should_panic(expected = "hold nothing")]
fn a_channel_that_can_hold_nothing_is_refused() {
    let _: Channel<Happening> = Channel::bounded(0, WhenFull::DropOldest);
}
