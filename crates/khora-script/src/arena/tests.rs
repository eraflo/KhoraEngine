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

//! Arena tests.
//!
//! The one that matters: a reference held past a reset must be **detected**,
//! not silently followed into whatever landed there next. Freeing a frame's
//! memory wholesale is only safe because that case is caught.

use super::{Arena, ArenaError, Object, Persisted, PersistentStore};
use crate::vm::Value;

fn array(values: &[i64]) -> Object {
    Object::Array(values.iter().map(|n| Value::Int(*n)).collect())
}

#[test]
fn an_allocated_object_reads_back() {
    let mut arena = Arena::new();
    let reference = arena
        .alloc(array(&[1, 2, 3]))
        .expect("room in a fresh arena");

    assert_eq!(arena.get(reference), Ok(&array(&[1, 2, 3])));
    assert_eq!(arena.len(), 1);
}

#[test]
fn objects_are_independent() {
    let mut arena = Arena::new();
    let first = arena.alloc(array(&[1])).expect("room");
    let second = arena.alloc(array(&[2])).expect("room");

    assert_eq!(arena.get(first), Ok(&array(&[1])));
    assert_eq!(arena.get(second), Ok(&array(&[2])));
}

#[test]
fn an_object_can_be_modified_in_place() {
    let mut arena = Arena::new();
    let reference = arena.alloc(array(&[1])).expect("room");

    if let Ok(Object::Array(values)) = arena.get_mut(reference) {
        values.push(Value::Int(2));
    }
    assert_eq!(arena.get(reference), Ok(&array(&[1, 2])));
}

/// **The guard.** A reference from a previous frame must not read whatever
/// landed at its index afterwards — that is a use-after-free with none of the
/// noise, and it is the one mistake this design can make.
#[test]
fn a_reference_from_a_previous_frame_is_detected() {
    let mut arena = Arena::new();
    let stale = arena.alloc(array(&[1, 2, 3])).expect("room");

    arena.reset();

    // Something else now occupies the same index.
    let fresh = arena.alloc(array(&[99])).expect("room");

    assert!(
        matches!(arena.get(stale), Err(ArenaError::Stale { .. })),
        "the old reference must not read the new object"
    );
    assert_eq!(arena.get(fresh), Ok(&array(&[99])), "the new one is fine");
}

/// The message says how long ago, and the note says what to do instead — a
/// bare "invalid reference" would leave the author guessing at the lifetime
/// rule.
#[test]
fn the_stale_error_explains_the_lifetime_rule() {
    let mut arena = Arena::new();
    let stale = arena.alloc(array(&[1])).expect("room");
    arena.reset();
    arena.reset();

    let Err(error) = arena.get(stale) else {
        panic!("expected the reference to be rejected");
    };
    assert!(
        error.message().contains("2 frame"),
        "got: {}",
        error.message()
    );
    assert!(
        error.note().contains("behavior field"),
        "got: {}",
        error.note()
    );
}

/// Resetting frees everything at once — one counter bump, whatever was
/// allocated. Nothing is traced and nothing is scanned.
#[test]
fn resetting_frees_everything_and_moves_the_generation() {
    let mut arena = Arena::new();
    for _ in 0..100 {
        arena.alloc(array(&[1])).expect("room");
    }
    assert_eq!(arena.len(), 100);

    let before = arena.generation();
    arena.reset();

    assert!(arena.is_empty());
    assert_ne!(arena.generation(), before);
}

/// A default-constructed reference cannot alias a live object: generation 0 is
/// reserved for "never allocated" and the arena starts at 1.
#[test]
fn generation_zero_is_never_live() {
    let arena = Arena::new();
    assert_ne!(arena.generation(), 0);
}

/// A script looping on `[…]` must fault one behavior rather than take the
/// process down.
#[test]
fn a_full_arena_refuses_rather_than_growing_without_bound() {
    let mut arena = Arena::new();
    let mut allocations = 0;

    loop {
        match arena.alloc(Object::Array(Vec::new())) {
            Ok(_) => {
                allocations += 1;
                assert!(allocations < 200_000, "the arena never filled");
            }
            Err(error) => {
                assert_eq!(error, ArenaError::Full);
                assert!(error.note().contains("without bound"));
                break;
            }
        }
    }
}

/// The arena travels with a saved scene, so it round-trips — including the
/// generation, or every reference would come back stale.
#[test]
fn an_arena_survives_serialization() {
    let mut arena = Arena::new();
    let reference = arena.alloc(array(&[4, 5])).expect("room");
    arena.reset();
    let live = arena.alloc(array(&[7])).expect("room");

    let json = serde_json::to_string(&arena).expect("serialises");
    let revived: Arena = serde_json::from_str(&json).expect("deserialises");

    assert_eq!(revived, arena);
    assert_eq!(revived.get(live), Ok(&array(&[7])));
    assert!(
        matches!(revived.get(reference), Err(ArenaError::Stale { .. })),
        "staleness survives the round trip too"
    );
}

#[test]
fn a_string_reports_its_length_in_characters() {
    let text = Object::Str("héllo".to_owned());
    assert_eq!(text.len(), 5, "characters, not the six bytes");
    assert_eq!(text.type_name(), "string");
}

#[test]
fn an_empty_object_says_so() {
    assert!(Object::Array(Vec::new()).is_empty());
    assert!(Object::Str(String::new()).is_empty());
    assert!(!array(&[1]).is_empty());
}

/// The two zones together: what the arena drops, the persistent store keeps —
/// and it keeps a *copy*, because a handle would be stale by the next frame.
#[test]
fn the_persistent_store_outlives_what_the_arena_drops() {
    let mut arena = Arena::new();
    let mut store = PersistentStore::with_slots(1);

    let reference = arena.alloc(array(&[1, 2, 3])).expect("room");
    let object = arena.get(reference).expect("just allocated").clone();
    store.store_object(0, &object);

    arena.reset();

    assert!(matches!(
        arena.get(reference),
        Err(ArenaError::Stale { .. })
    ));
    assert_eq!(
        store.get(0),
        Some(&Persisted::Owned(array(&[1, 2, 3]))),
        "the copy survived the frame the original did not"
    );
}
