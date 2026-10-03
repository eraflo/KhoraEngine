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

//! A snapshot is the same build's cache: anything it holds that this build
//! did not write — another schema, a name not registered exactly so, a
//! component that is not saved, a value that is not exactly its bytes, a
//! damaged file — refuses the whole load, and the world stays as it was.

use khora_core::math::Vec3;
use khora_core::scene::SceneFile;
use khora_data::ecs::{ActiveEvents, Collider, Name, Parent, Transform};
use khora_data::scene::snapshot::prepare_snapshot;
use khora_data::scene::{registration_of, write_scene_file, CompactEncoding};

use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// One entity as a test sees it: identity, name, translation, parent's
/// identity.
type Observed = (
    PersistentId,
    Option<String>,
    Option<Vec3>,
    Option<PersistentId>,
);

/// What a test can observe of a world, sorted by identity.
fn observed(world: &World) -> Vec<Observed> {
    let mut seen: Vec<_> = world
        .iter_entities()
        .map(|e| {
            (
                world.persistent_id(e).expect("every entity has an id"),
                world.get::<Name>(e).map(|n| n.as_str().to_owned()),
                world.get::<Transform>(e).map(|t| t.translation),
                world
                    .get::<Parent>(e)
                    .and_then(|p| world.persistent_id(p.0)),
            )
        })
        .collect();
    seen.sort_by_key(|(id, ..)| *id);
    seen
}

/// A small authored scene: a parent and a child, with names.
fn named_world(prefix: &str) -> World {
    let mut world = World::new();
    let root = world.spawn((
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
        Name::new(format!("{prefix} root")),
    ));
    let child = world.spawn((
        Transform::from_translation(Vec3::new(0.0, 0.1, 0.0)),
        Name::new(format!("{prefix} child")),
    ));
    world.set_parent(child, Some(root));
    for entity in [root, child] {
        world.mark_authored(entity).expect("authored");
    }
    world
}

/// `file` with its payload replaced, the header's length kept in step.
fn with_payload(file: &SceneFile, payload: Vec<u8>) -> SceneFile {
    let mut header = file.header.clone();
    header.payload_length = payload.len() as u64;
    SceneFile { header, payload }
}

/// Where `needle` sits in `payload`, asserting it sits there exactly once.
fn only_place(payload: &[u8], needle: &[u8]) -> usize {
    let places: Vec<usize> = payload
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle)
        .map(|(at, _)| at)
        .collect();
    assert_eq!(
        places.len(),
        1,
        "`{}` is expected once in the snapshot, found at {places:?}",
        String::from_utf8_lossy(needle)
    );
    places[0]
}

/// `file` with the one occurrence of `from` replaced by `to`, of the same
/// length.
fn replaced(file: &SceneFile, from: &str, to: &str) -> SceneFile {
    assert_eq!(from.len(), to.len(), "a same-length replacement");
    let mut payload = file.payload.clone();
    let at = only_place(&payload, from.as_bytes());
    payload[at..at + to.len()].copy_from_slice(to.as_bytes());
    with_payload(file, payload)
}

/// Asserts loading `file` fails, naming `named`, and leaves an open world
/// exactly as it was.
fn assert_refused(file: &SceneFile, named: &str, case: &str) {
    let mut world = named_world("Open");
    let before = observed(&world);
    let count = world.iter_entities().count();
    let failure = match prepare_snapshot(&mut world, file) {
        Ok(prepared) => {
            prepared.abandon(&mut world);
            panic!("{case}: the snapshot was accepted");
        }
        Err(failure) => failure,
    };
    assert!(
        failure.message.contains(named),
        "{case}: the refusal names `{named}`: {}",
        failure.message
    );
    assert_eq!(world.iter_entities().count(), count, "{case}: entity count");
    assert_eq!(observed(&world), before, "{case}: the world changed");
}

/// A component whose schema fingerprint is not this build's — a field
/// renamed, retyped, reordered, a variant changed since the snapshot was
/// written — refuses the whole snapshot, naming the component.
#[test]
fn a_snapshot_from_another_schema_is_refused() {
    let file = snapshot_of(&named_world("Saved"));
    for name in ["Transform", "Name"] {
        let reg = registration_of(name).expect("registered");
        let fingerprint = (reg.schema)().to_le_bytes();
        let mut payload = file.payload.clone();
        let at = only_place(&payload, &fingerprint);
        payload[at] ^= 0x01;
        assert_refused(
            &with_payload(&file, payload),
            name,
            &format!("{name}'s fingerprint"),
        );
    }
}

/// A component name nobody registered is refused, naming it.
#[test]
fn an_unknown_component_in_a_snapshot_is_refused() {
    let file = snapshot_of(&named_world("Saved"));
    assert_refused(
        &replaced(&file, "Transform", "Transfxrm"),
        "Transfxrm",
        "an unknown name",
    );
}

/// A name a component was known by before is not this build's name for it:
/// a snapshot reads no `formerly`.
#[test]
fn a_former_name_in_a_snapshot_is_refused() {
    let mut src = World::new();
    src.spawn((Transform::identity(), ActiveEvents));
    let file = snapshot_of(&src);
    // `LegacyBeacon` is what `Beacon` (`renames.rs`) was called; it has the
    // length of `ActiveEvents`.
    assert_refused(
        &replaced(&file, "ActiveEvents", "LegacyBeacon"),
        "LegacyBeacon",
        "a former name",
    );
}

/// A component the engine derives is never saved; a snapshot naming one is
/// refused rather than skipped.
#[test]
fn a_component_that_is_not_saved_is_refused() {
    let mut src = World::new();
    src.spawn((Transform::identity(), Collider::default()));
    let file = snapshot_of(&src);
    assert_refused(
        &replaced(&file, "Collider", "Children"),
        "Children",
        "a derived component",
    );
}

/// The snapshot of one entity holding only `Name("Alpha")`, with its one
/// value made a byte longer than the name reads: the value's length says
/// seven bytes where the name is six.
fn overlong_value() -> SceneFile {
    let mut src = World::new();
    src.spawn(Name::new("Alpha"));
    let file = snapshot_of(&src);
    let mut payload = file.payload.clone();
    let n = payload.len();
    // The last value of the last page: `varint length (6)`, then the name —
    // `varint length (5)` and its five bytes.
    assert!(payload.ends_with(b"Alpha"), "the name is the last value");
    assert_eq!(payload[n - 6], 5, "the name's length precedes it");
    assert_eq!(payload[n - 7], 6, "the value's length precedes the name");
    payload[n - 7] = 7;
    payload.push(0);
    with_payload(&file, payload)
}

/// A value whose bytes are not consumed exactly — one left over — is
/// refused, naming the component.
#[test]
fn a_value_that_is_not_exactly_its_bytes_is_refused() {
    assert_refused(&overlong_value(), "Name", "a byte left over");
}

/// A snapshot written by another version of the snapshot layout is refused.
#[test]
fn a_snapshot_of_another_layout_version_is_refused() {
    let file = snapshot_of(&named_world("Saved"));
    let mut payload = file.payload.clone();
    payload[0] ^= 0xFF;
    let mut world = named_world("Open");
    let before = observed(&world);
    assert!(prepare_snapshot(&mut world, &with_payload(&file, payload)).is_err());
    assert_eq!(observed(&world), before);
}

/// `prepare_snapshot` reads snapshots only: a file in a record encoding is
/// refused, not guessed at.
#[test]
fn a_file_in_another_encoding_is_not_a_snapshot() {
    let record = capture_world(&named_world("Saved")).expect("captures");
    let compact = write_scene_file(&record, &CompactEncoding).expect("encodes");
    let mut world = named_world("Open");
    let before = observed(&world);
    assert!(prepare_snapshot(&mut world, &compact).is_err());
    assert_eq!(observed(&world), before);
}

/// A damaged snapshot is an error, never a panic, and changes nothing: every
/// prefix of a real one, every byte of it flipped, and bytes that were never
/// a snapshot.
#[test]
fn a_damaged_snapshot_is_an_error_not_a_panic() {
    let (sample, _) = super::sample::sample_world();
    let file = snapshot_of(&sample);
    let mut world = named_world("Open");
    let before = observed(&world);

    for end in 0..file.payload.len() {
        let cut = with_payload(&file, file.payload[..end].to_vec());
        assert!(
            prepare_snapshot(&mut world, &cut).is_err(),
            "a prefix of {end} bytes was accepted"
        );
    }
    for at in 0..file.payload.len() {
        let mut payload = file.payload.clone();
        payload[at] ^= 0xFF;
        if let Ok(prepared) = prepare_snapshot(&mut world, &with_payload(&file, payload)) {
            prepared.abandon(&mut world);
        }
    }
    for garbage in [vec![0xFF; 64], vec![0; 64], vec![1; 3], Vec::new()] {
        assert!(
            prepare_snapshot(&mut world, &with_payload(&file, garbage.clone())).is_err(),
            "{garbage:?} was accepted"
        );
    }
    assert_eq!(
        observed(&world),
        before,
        "a refused snapshot changed the world"
    );
}

/// A load refused part-way gives back every id it reserved: a hundred of
/// them leave the next spawn with a small index.
#[test]
fn a_refused_snapshot_releases_its_reservations() {
    let file = overlong_value();
    let mut world = World::new();
    for round in 0..100 {
        assert!(
            load_snapshot(&mut world, &file).is_err(),
            "round {round}: the overlong value was accepted"
        );
    }
    assert_eq!(world.iter_entities().count(), 0);
    let next = world.spawn(Transform::identity());
    assert!(
        next.index < 8,
        "a hundred refused loads left reserved slots behind: {next:?}"
    );
}
