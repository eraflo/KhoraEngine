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

//! Bytes that are not what the type wrote: an error, never a panic, a hang
//! or an allocation the input cannot pay for.

use crate::scene::record::MAX_DEPTH;

use super::*;

/// A value cut short anywhere is refused: a positional value needs all of
/// its bytes.
#[test]
fn every_proper_prefix_is_an_error() {
    let bytes = encode(&rich_scene());
    assert!(bytes.len() > 16, "the sample is not trivially short");
    for end in 0..bytes.len() {
        assert!(
            decode::<Scene>(&bytes[..end]).is_err(),
            "a prefix of {end} of {} bytes was accepted",
            bytes.len()
        );
    }
}

/// The whole slice is the value: a byte left over is refused.
#[test]
fn trailing_bytes_are_an_error() {
    let mut bytes = encode(&rich_scene());
    bytes.push(0);
    assert!(decode::<Scene>(&bytes).is_err(), "one trailing byte");
    bytes.extend_from_slice(&[1, 2, 3, 4]);
    assert!(decode::<Scene>(&bytes).is_err(), "several trailing bytes");

    let mut number = encode(&7u32);
    number.push(0);
    assert!(decode::<u32>(&number).is_err(), "a number and a byte");
}

/// Counts no input can back.
fn huge_counts() -> Vec<Vec<u8>> {
    vec![
        // A malformed varint: every byte says another follows.
        vec![0xFF; 10],
        vec![0xFF; 32],
        // 2^40 as LEB128, then almost nothing.
        vec![0x80, 0x80, 0x80, 0x80, 0x80, 0x20, 1, 2, 3],
        // 1000 as LEB128, then three bytes.
        vec![0xE8, 0x07, 1, 2, 3],
        // u64::MAX as LEB128.
        vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
    ]
}

/// A length or count larger than what is left is refused before anything
/// is allocated or looped over — for elements of any size, none included.
#[test]
fn a_huge_count_is_an_error() {
    for bytes in huge_counts() {
        assert!(decode::<Vec<u64>>(&bytes).is_err(), "Vec<u64>: {bytes:?}");
        assert!(decode::<Vec<u8>>(&bytes).is_err(), "Vec<u8>: {bytes:?}");
        assert!(decode::<String>(&bytes).is_err(), "String: {bytes:?}");
        assert!(decode::<Blob>(&bytes).is_err(), "bytes: {bytes:?}");
        assert!(
            decode::<BTreeMap<u32, u32>>(&bytes).is_err(),
            "map: {bytes:?}"
        );
        // Zero bytes per element: only the bound on the count stops it.
        assert!(
            decode::<Vec<Marker>>(&bytes).is_err(),
            "Vec<Marker>: {bytes:?}"
        );
        assert!(decode::<Vec<()>>(&bytes).is_err(), "Vec<()>: {bytes:?}");
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum Wide {
    A,
    B,
    C,
    D,
}

#[derive(Debug, Serialize, Deserialize)]
enum Narrow {
    A,
    B,
}

/// A variant index the enum does not have is refused.
#[test]
fn an_invalid_variant_index_is_an_error() {
    let bytes = encode(&Wide::D);
    assert!(decode::<Narrow>(&bytes).is_err(), "variant 3 of 2");
    let bytes = encode(&Wide::B);
    assert!(decode::<Narrow>(&bytes).is_ok(), "variant 1 of 2 is fine");
}

/// A string whose bytes are not UTF-8 is refused.
#[test]
fn invalid_utf8_is_an_error() {
    let mut bytes = encode(&"ab".to_owned());
    let at = bytes
        .windows(2)
        .position(|window| window == b"ab")
        .expect("a string's bytes are written verbatim");
    bytes[at] = 0xC3;
    bytes[at + 1] = 0x28;
    assert!(decode::<String>(&bytes).is_err());
}

/// A byte that is neither tag a `bool` or an `Option` is written with is
/// refused, not read as one of them.
#[test]
fn an_invalid_tag_is_an_error() {
    assert_eq!(decode::<bool>(&encode(&true)), Ok(true));
    assert_eq!(decode::<bool>(&encode(&false)), Ok(false));
    assert!(decode::<bool>(&encode(&7u8)).is_err(), "bool from 7");
    assert!(decode::<Option<u8>>(&encode(&7u8)).is_err(), "option tag 7");
}

/// A value that nests itself, through an option.
#[derive(Debug, Serialize, Deserialize)]
struct Nest(Option<Box<Nest>>);

fn nest(depth: usize) -> Nest {
    let mut value = Nest(None);
    for _ in 0..depth {
        value = Nest(Some(Box::new(value)));
    }
    value
}

/// A modest nesting round-trips; one past `MAX_DEPTH` is refused on write,
/// and bytes claiming a nesting far deeper than any stack are refused on read
/// without exhausting it.
#[test]
fn depth_is_bounded() {
    let shallow = nest(20);
    let back = round_trip(&shallow);
    assert_eq!(format!("{back:?}"), format!("{shallow:?}"));

    let mut out = Vec::new();
    assert!(
        to_positional(&nest(MAX_DEPTH * 2 + 8), &mut out, &mut Table::default()).is_err(),
        "a value nesting past MAX_DEPTH is written"
    );

    // Each level of `nest` adds the same bytes in front of the one inside it.
    let inner = encode(&nest(0));
    let one = encode(&nest(1));
    let two = encode(&nest(2));
    assert!(one.ends_with(&inner), "a level wraps the value inside it");
    let level = one[..one.len() - inner.len()].to_vec();
    assert_eq!(two, [level.as_slice(), &one].concat(), "levels repeat");

    let mut deep = level.repeat(100_000);
    deep.extend_from_slice(&inner);
    assert!(
        decode::<Nest>(&deep).is_err(),
        "a nesting of 100 000 levels was read"
    );
}

/// Bytes that are anything at all never panic: a deterministic spray of
/// inputs, read as a value of every kind.
#[test]
fn arbitrary_bytes_never_panic() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for length in 0..96 {
        for _ in 0..24 {
            let bytes: Vec<u8> = (0..length).map(|_| next() as u8).collect();
            let _ = decode::<Scene>(&bytes);
            let _ = decode::<Vec<Shape>>(&bytes);
            let _ = decode::<Option<String>>(&bytes);
            let _ = decode::<Numbers>(&bytes);
        }
    }
    // And bytes a real value wrote, with one byte flipped at a time.
    let good = encode(&rich_scene());
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0xFF;
        let _ = decode::<Scene>(&bad);
    }
}
