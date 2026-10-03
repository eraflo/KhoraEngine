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

//! A load runs on whatever thread the application loads from — a Windows
//! main thread has 1 MiB of stack, and the editor runs unoptimised builds.
//! Reading the deepest value a record may hold must fit in it, in every
//! encoding.

use khora_data::scene::EncodingError;

use super::*;

/// Reads a scene record from bytes, in one encoding.
type Decode = fn(&[u8]) -> Result<SceneRecord, EncodingError>;

/// A value `depth` lists deep.
fn nested(depth: usize) -> Record {
    let mut value = Record::U64(1);
    for _ in 0..depth {
        value = Record::Seq(vec![value]);
    }
    value
}

/// The deepest value a record holds decodes on a 1 MiB thread, whichever
/// encoding wrote it.
#[test]
fn the_deepest_value_decodes_on_a_one_mib_stack() {
    let record = SceneRecord {
        entities: vec![PersistentId::authored(7)],
        pages: vec![PageRecord {
            components: vec!["Transform".into()],
            rows: vec![PersistentId::authored(7)],
            columns: vec![vec![nested(128)]],
        }],
    };
    let decoders: [(&str, Decode); 3] = [
        ("compact", |bytes| CompactEncoding.decode(bytes)),
        ("text", |bytes| TextEncoding.decode(bytes)),
        ("msgpack", |bytes| MsgPackEncoding.decode(bytes)),
    ];
    for ((name, encoding), (_, decode)) in every_encoding().into_iter().zip(decoders) {
        let bytes = encoding.encode(&record).expect("encodes");
        let outcome = std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || decode(&bytes).map(|_| ()))
            .expect("spawns")
            .join();
        assert!(matches!(outcome, Ok(Ok(()))), "{name}: {outcome:?}");
    }
}

/// One level past it is refused, in text as in the binary encodings.
#[test]
fn one_level_past_the_bound_is_refused_in_text_too() {
    let record = SceneRecord {
        entities: vec![PersistentId::authored(7)],
        pages: vec![PageRecord {
            components: vec!["Transform".into()],
            rows: vec![PersistentId::authored(7)],
            columns: vec![vec![nested(129)]],
        }],
    };
    let bytes = TextEncoding.encode(&record).expect("encodes");
    assert!(TextEncoding.decode(&bytes).is_err());
}
