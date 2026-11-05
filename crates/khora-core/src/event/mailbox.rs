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

//! A family's mail: the [`Channel`]s a family reads, and what becomes of their
//! unread items while the family is off.
//!
//! A channel is shared by whoever sends to it; whether its mail outlives a
//! session of its reader is a fact about the reader, declared once beside it:
//!
//! ```ignore
//! inventory::submit! {
//!     Mailbox::of::<ScriptEvent>(SCRIPT, Reader::Take, BetweenSessions::Drop)
//! }
//! ```
//!
//! At the frame boundary, a family off for the coming frame has its `Drop`
//! mailboxes emptied ([`discard_mail_of_inactive`]): what was sent to it while
//! it was off never reaches its next session.

use std::collections::BTreeSet;

use crate::agent::gorna::AgentId;
use crate::agent::Activation;
use crate::runtime::Runtime;

use super::{Channel, Supersedes};

/// How a family reads its mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// It takes the items: nobody else reads them.
    Take,
    /// It reads them by the named cursor
    /// ([`Channel::read_for`]); other readers keep theirs.
    Cursor(&'static str),
}

/// What becomes of a mailbox's unread items while its family is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BetweenSessions {
    /// They are discarded: mail sent for one session is not delivered to the
    /// next.
    Drop,
    /// They wait for the family to come back on.
    Hold,
}

/// One family's mailbox: a channel it reads, and how.
pub struct Mailbox {
    /// The family reading it.
    pub family: AgentId,
    /// How it reads it.
    pub reader: Reader,
    /// What becomes of its unread mail while the family is off.
    pub between_sessions: BetweenSessions,
    /// The channel's item type, by name, for diagnostics.
    pub message: fn() -> &'static str,
    /// Discards what `reader` has not read of the runtime's `Channel<T>`.
    empty: fn(&Runtime, Reader),
}

impl Mailbox {
    /// The mailbox of `family` on the runtime's `Channel<T>`.
    pub const fn of<T: Supersedes + Clone + Send + Sync + 'static>(
        family: AgentId,
        reader: Reader,
        between_sessions: BetweenSessions,
    ) -> Self {
        Self {
            family,
            reader,
            between_sessions,
            message: std::any::type_name::<T>,
            empty: empty_channel::<T>,
        }
    }

    /// Discards what this mailbox's reader has not read of `runtime`'s
    /// channel: everything for a [`Reader::Take`], the named reader's unread
    /// items for a [`Reader::Cursor`]. A runtime without the channel holds
    /// nothing to discard.
    pub fn empty(&self, runtime: &Runtime) {
        (self.empty)(runtime, self.reader);
    }
}

impl std::fmt::Debug for Mailbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailbox")
            .field("family", &self.family)
            .field("reader", &self.reader)
            .field("between_sessions", &self.between_sessions)
            .field("message", &(self.message)())
            .finish()
    }
}

inventory::collect!(Mailbox);

fn empty_channel<T: Supersedes + Clone + Send + Sync + 'static>(runtime: &Runtime, reader: Reader) {
    let Some(channel) = runtime.resources.get::<Channel<T>>() else {
        return;
    };
    match reader {
        Reader::Take => channel.discard_unread(),
        Reader::Cursor(name) => channel.skip_to_end(name),
    }
}

/// Empties every [`BetweenSessions::Drop`] mailbox of a family off in
/// `activation`'s state in force, so a family off for the coming frame starts
/// its next session with no mail of the last.
pub fn discard_mail_of_inactive(activation: &Activation, runtime: &Runtime) {
    discard_mail_of(&activation.inactive(), runtime);
}

/// Empties every [`BetweenSessions::Drop`] mailbox of the `families` named.
///
/// What the frame boundary does, once it has settled, for every family off in
/// the frame that ended or in the coming one: what was sent to a family while
/// it was off — its last frame off included, though the boundary that ends it
/// switches the family on — never reaches its next session.
pub fn discard_mail_of(families: &BTreeSet<AgentId>, runtime: &Runtime) {
    if families.is_empty() {
        return;
    }
    for mailbox in inventory::iter::<Mailbox> {
        if mailbox.between_sessions == BetweenSessions::Drop && families.contains(&mailbox.family) {
            mailbox.empty(runtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::WhenFull;

    const LISTENER: AgentId = AgentId::new("mailbox_test_listener");
    const KEEPER: AgentId = AgentId::new("mailbox_test_keeper");
    const WATCHER: AgentId = AgentId::new("mailbox_test_watcher");

    /// Taken by its family, dropped between sessions.
    #[derive(Debug, Clone, PartialEq)]
    struct Taken(u32);
    impl Supersedes for Taken {}

    /// Taken by its family, held between sessions.
    #[derive(Debug, Clone, PartialEq)]
    struct Kept(u32);
    impl Supersedes for Kept {}

    /// Read by its family's named cursor, dropped between sessions.
    #[derive(Debug, Clone, PartialEq)]
    struct Watched(u32);
    impl Supersedes for Watched {}

    inventory::submit! {
        Mailbox::of::<Taken>(LISTENER, Reader::Take, BetweenSessions::Drop)
    }
    inventory::submit! {
        Mailbox::of::<Kept>(KEEPER, Reader::Take, BetweenSessions::Hold)
    }
    inventory::submit! {
        Mailbox::of::<Watched>(WATCHER, Reader::Cursor("watcher"), BetweenSessions::Drop)
    }

    /// The three channels, each holding two items, in one runtime.
    fn runtime_with_mail() -> Runtime {
        let mut runtime = Runtime::new();
        let taken = Channel::<Taken>::bounded(8, WhenFull::DropOldest);
        let kept = Channel::<Kept>::bounded(8, WhenFull::DropOldest);
        let watched = Channel::<Watched>::bounded(8, WhenFull::DropOldest);
        for n in 0..2 {
            taken.send(Taken(n));
            kept.send(Kept(n));
            watched.send(Watched(n));
        }
        runtime.resources.insert(taken);
        runtime.resources.insert(kept);
        runtime.resources.insert(watched);
        runtime
    }

    fn channel<T: Supersedes + Send + Sync + 'static>(runtime: &Runtime) -> &Channel<T> {
        runtime.resources.get::<Channel<T>>().expect("the channel")
    }

    /// **A family off for the coming frame loses its `Drop` mail, and keeps
    /// its `Hold` mail.** The taken channel is emptied, counted as discarded
    /// and not as overflow; the held one keeps its items; the named reader of
    /// the watched channel is moved past its unread items, while another
    /// reader of the same channel keeps them.
    #[test]
    fn a_family_switched_off_loses_its_drop_mail_and_keeps_its_hold_mail() {
        let runtime = runtime_with_mail();
        let activation = Activation::new([LISTENER, KEEPER, WATCHER]);
        for family in [LISTENER, KEEPER, WATCHER] {
            activation.deactivate(family).expect("a known family");
        }
        activation.settle();

        discard_mail_of_inactive(&activation, &runtime);

        let taken = channel::<Taken>(&runtime);
        assert!(taken.is_empty(), "the taken mail is discarded");
        assert_eq!(taken.discarded(), 2);
        assert_eq!(taken.dropped(), 0, "a discard is no overflow");

        assert_eq!(
            channel::<Kept>(&runtime).len(),
            2,
            "the held mail waits for the family"
        );

        let watched = channel::<Watched>(&runtime);
        assert!(
            watched.read_for("watcher").is_empty(),
            "the family's reader is past its unread mail"
        );
        assert_eq!(
            watched.read_for("someone_else"),
            vec![Watched(0), Watched(1)],
            "another reader of the channel keeps it"
        );
        assert_eq!(watched.discarded(), 2);
    }

    /// **A family on keeps its mail**, `Drop` or not: only a family off for
    /// the coming frame is emptied — here the listener, not the watcher.
    #[test]
    fn a_family_on_keeps_its_mail() {
        let runtime = runtime_with_mail();
        let activation = Activation::new([LISTENER, KEEPER, WATCHER]);
        activation.deactivate(LISTENER).expect("a known family");
        activation.settle();

        discard_mail_of_inactive(&activation, &runtime);

        assert!(channel::<Taken>(&runtime).is_empty(), "the listener is off");
        assert_eq!(
            channel::<Watched>(&runtime).read_for("watcher"),
            vec![Watched(0), Watched(1)],
            "the watcher is on: its mail is delivered"
        );
        assert_eq!(channel::<Watched>(&runtime).discarded(), 0);
    }

    /// **A request not yet settled empties nothing**: the boundary empties
    /// the mail of a family off in the state in force.
    #[test]
    fn a_request_not_settled_empties_nothing() {
        let runtime = runtime_with_mail();
        let activation = Activation::new([LISTENER, KEEPER, WATCHER]);
        activation.deactivate(LISTENER).expect("a known family");

        discard_mail_of_inactive(&activation, &runtime);

        assert_eq!(channel::<Taken>(&runtime).len(), 2);
    }

    /// **The families named lose their `Drop` mail, whatever is in force**:
    /// the boundary also names a family it just switched on, whose last frame
    /// off may have been sent mail.
    #[test]
    fn the_families_named_lose_their_drop_mail_whatever_is_in_force() {
        let runtime = runtime_with_mail();

        discard_mail_of(&BTreeSet::from([LISTENER, KEEPER]), &runtime);

        assert!(channel::<Taken>(&runtime).is_empty(), "named, dropped");
        assert_eq!(channel::<Kept>(&runtime).len(), 2, "named, but held");
        assert_eq!(
            channel::<Watched>(&runtime).read_for("watcher"),
            vec![Watched(0), Watched(1)],
            "not named"
        );
    }

    /// **`Mailbox::empty` reaches the runtime's channel** through the
    /// mailbox's own reader.
    #[test]
    fn a_mailbox_empties_its_channel_by_its_reader() {
        let runtime = runtime_with_mail();
        let taken = inventory::iter::<Mailbox>
            .into_iter()
            .find(|mailbox| mailbox.family == LISTENER)
            .expect("the listener's mailbox is registered");

        taken.empty(&runtime);

        assert!(channel::<Taken>(&runtime).is_empty());
        assert_eq!(channel::<Kept>(&runtime).len(), 2, "only its own channel");
    }
}
