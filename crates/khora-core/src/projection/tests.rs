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

//! An [`Interest`]: each owner's announcement, their union, and the
//! generation that moves with the union alone.

use std::sync::Arc;

use super::{Demand, Interest};

/// A demand for names, as a Flow projecting named columns has.
struct Names;

impl Demand for Names {
    type Item = &'static str;
}

/// A demand for numbers, for the threads.
struct Numbers;

impl Demand for Numbers {
    type Item = u32;
}

/// **A fresh interest wants nothing.** Its union is empty, it covers an empty
/// list — asking for nothing is always answered — and nothing else.
#[test]
fn a_fresh_interest_wants_nothing_and_covers_only_the_empty_list() {
    let interest = Interest::<Names>::new();

    assert!(interest.wanted().is_empty(), "{:?}", interest.wanted());
    assert!(interest.covers(&[]), "nothing asked is covered");
    assert!(!interest.covers(&["Collider"]), "nobody wants the collider");
}

/// **What is wanted is every owner's items together, sorted, each once**,
/// however each owner listed them.
#[test]
fn wanted_is_the_sorted_union_of_every_owner_each_once() {
    let interest = Interest::<Names>::new();

    interest.announce("loaded", ["RigidBody", "Collider", "RigidBody"]);
    interest.announce("module:ai/guard.erg", ["Light", "Collider"]);

    assert_eq!(interest.wanted(), vec!["Collider", "Light", "RigidBody"]);
}

/// **An owner's new announcement replaces its old one**: a module saved twice
/// before its first version installed wants only what its second version
/// reads, and an owner announcing nothing wants nothing.
#[test]
fn an_owners_new_announcement_replaces_its_entry() {
    let interest = Interest::<Names>::new();

    interest.announce("module:ai/guard.erg", ["Collider"]);
    interest.announce("module:ai/guard.erg", ["RigidBody"]);
    assert_eq!(
        interest.wanted(),
        vec!["RigidBody"],
        "the second replaced the first"
    );

    interest.announce("module:ai/guard.erg", []);
    assert!(
        interest.wanted().is_empty(),
        "an empty announcement wants nothing: {:?}",
        interest.wanted()
    );
}

/// **Withdrawing an owner drops its items, and only its.** Another owner
/// keeps what it announced; withdrawing an owner that announced nothing
/// changes nothing.
#[test]
fn withdrawing_an_owner_drops_its_items_alone() {
    let interest = Interest::<Names>::new();
    interest.announce("module:ai/guard.erg", ["Collider"]);
    interest.announce("module:ai/lamp.erg", ["Light"]);

    interest.withdraw("module:ai/guard.erg");
    assert_eq!(interest.wanted(), vec!["Light"]);

    interest.withdraw("module:ai/door.erg");
    assert_eq!(
        interest.wanted(),
        vec!["Light"],
        "an absent owner leaves it as it is"
    );
}

/// **An owner announcing touches no other owner's entry.** The installed
/// programs' reads, announced again every frame, do not erase what an
/// incoming module announced: its program is not installed yet, and the flow
/// must go on projecting what it will read.
#[test]
fn an_owner_announcing_leaves_the_other_owners_entries() {
    let interest = Interest::<Names>::new();
    interest.announce("module:ai/guard.erg", ["Collider"]);

    interest.announce("loaded", ["RigidBody"]);
    assert_eq!(interest.wanted(), vec!["Collider", "RigidBody"]);

    interest.announce("loaded", []);
    assert_eq!(
        interest.wanted(),
        vec!["Collider"],
        "the installed programs reading nothing leaves the module's entry"
    );
}

/// **An item two owners hold stays wanted while one still holds it.** The
/// installed program reads the collider, and so does the incoming version of
/// its module: installing it withdraws the module's entry, and the collider is
/// still projected.
#[test]
fn an_item_two_owners_hold_stays_wanted_while_one_holds_it() {
    let interest = Interest::<Names>::new();
    interest.announce("loaded", ["Collider"]);
    interest.announce("module:ai/guard.erg", ["Collider", "RigidBody"]);

    interest.withdraw("module:ai/guard.erg");

    assert_eq!(interest.wanted(), vec!["Collider"]);
}

/// **An interest covers a list when every item of it is wanted** — by any
/// owner, an item named twice as well as once — and not when one item is
/// missing.
#[test]
fn covers_is_true_only_when_every_item_is_wanted() {
    let interest = Interest::<Names>::new();
    interest.announce("loaded", ["RigidBody"]);
    interest.announce("module:ai/guard.erg", ["Collider"]);

    assert!(interest.covers(&["Collider"]));
    assert!(interest.covers(&["RigidBody", "Collider"]), "across owners");
    assert!(interest.covers(&["Collider", "Collider"]), "named twice");
    assert!(interest.covers(&[]), "nothing asked");
    assert!(
        !interest.covers(&["Collider", "Light"]),
        "the light is wanted by nobody"
    );

    interest.withdraw("module:ai/guard.erg");
    assert!(
        !interest.covers(&["Collider"]),
        "withdrawn, the collider is no longer covered"
    );
}

/// **The generation moves on every change of the union**, to one not seen
/// before — even when the union comes back to a set it was once: a Flow
/// keyed on it projects again whenever what is wanted changed.
#[test]
fn the_generation_moves_on_every_change_of_the_union() {
    let interest = Interest::<Names>::new();
    let mut seen = vec![interest.generation()];
    let mut moved = |interest: &Interest<Names>, what: &str| {
        let generation = interest.generation();
        assert!(
            !seen.contains(&generation),
            "{what}: the generation {generation} was seen before: {seen:?}"
        );
        seen.push(generation);
    };

    interest.announce("loaded", ["RigidBody"]);
    moved(&interest, "a first announcement");
    interest.announce("module:ai/guard.erg", ["Collider"]);
    moved(&interest, "a second owner with a new item");
    interest.announce("loaded", ["Light"]);
    moved(&interest, "an owner replacing its item");
    interest.withdraw("module:ai/guard.erg");
    moved(&interest, "a withdrawal taking an item away");
    interest.withdraw("loaded");
    moved(&interest, "the union emptied, as it started");
}

/// **The generation stays when the union does not change**: the same items
/// announced again (reordered, or with a duplicate), an owner withdrawn that
/// announced nothing, a new owner wanting only what is already wanted, one of
/// two owners of an item leaving, and a new owner wanting nothing.
#[test]
fn the_generation_stays_when_the_union_does_not_change() {
    let interest = Interest::<Names>::new();
    interest.announce("loaded", ["Collider", "RigidBody"]);
    let generation = interest.generation();
    let stays = |interest: &Interest<Names>, what: &str| {
        assert_eq!(
            interest.generation(),
            generation,
            "{what} moved the generation"
        );
    };

    interest.announce("loaded", ["RigidBody", "Collider", "Collider"]);
    stays(&interest, "the same items again");
    interest.withdraw("module:ai/door.erg");
    stays(&interest, "withdrawing an absent owner");
    interest.announce("module:ai/guard.erg", ["Collider"]);
    stays(&interest, "a new owner of an item already wanted");
    interest.withdraw("module:ai/guard.erg");
    stays(&interest, "one of two owners of an item leaving");
    interest.announce("module:ai/lamp.erg", []);
    stays(&interest, "a new owner wanting nothing");

    assert_eq!(interest.wanted(), vec!["Collider", "RigidBody"]);
}

/// **Owners announce from many threads at once**, as the hot-reload pump and
/// the frame's DataSystems do: every announcement lands, the union is every
/// owner's last one, and a reader meanwhile always reads a sorted union.
#[test]
fn announcing_from_many_threads_ends_with_the_union_of_all() {
    let interest = Arc::new(Interest::<Numbers>::new());
    let start = interest.generation();

    let writers: Vec<_> = (0..8u32)
        .map(|thread| {
            let interest = Arc::clone(&interest);
            std::thread::spawn(move || {
                for round in 0..50u32 {
                    // Each round replaces the thread's own entry; the last one
                    // stands.
                    let items = [thread * 2, thread * 2 + 1, 100 + round % 3];
                    interest.announce(format!("thread {thread}"), items);
                }
                interest.announce(format!("thread {thread}"), [thread * 2, thread * 2 + 1]);
            })
        })
        .collect();
    let reader = {
        let interest = Arc::clone(&interest);
        std::thread::spawn(move || {
            for _ in 0..200 {
                let wanted = interest.wanted();
                assert!(
                    wanted.windows(2).all(|pair| pair[0] < pair[1]),
                    "read mid-announcement, still sorted and each once: {wanted:?}"
                );
            }
        })
    };
    for writer in writers {
        writer.join().expect("a writer panicked");
    }
    reader.join().expect("the reader panicked");

    assert_eq!(interest.wanted(), (0..16).collect::<Vec<u32>>());
    assert_ne!(interest.generation(), start, "the union changed");
}
