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

//! Numbers read into `f32` fields: the decimal a text save holds, and the
//! double a binary one holds.

use khora_core::math::Vec3;
use khora_data::ecs::{Transform, World};
use khora_data::scene::record::Record;

use super::renames::Beacon;
use super::*;

/// A world holding `f32`s that are not binary fractions: tenths.
fn tenths() -> (World, PersistentId) {
    let mut world = World::new();
    let entity = world.spawn((
        Transform::from_translation(Vec3::new(0.1, 0.2, 0.3)),
        Beacon { range: 0.1 },
    ));
    let id = world.mark_authored(entity).expect("authored");
    (world, id)
}

/// The `range` field of the beacon `record` holds for `id` — a struct's field
/// as written, or a map entry keyed by the field's name once read back from a
/// self-describing form.
fn range_mut(record: &mut SceneRecord, id: PersistentId) -> &mut Record {
    match value_mut(record, id, "Beacon") {
        Record::Struct { fields, .. } => {
            &mut fields
                .iter_mut()
                .find(|(name, _)| name == "range")
                .expect("the field is recorded by name")
                .1
        }
        Record::Map(entries) => {
            &mut entries
                .iter_mut()
                .find(|(key, _)| *key == Record::Str("range".into()))
                .expect("the field is recorded by name")
                .1
        }
        other => panic!("a struct component is a struct or a map: {other:?}"),
    }
}

/// A text save writes an `f32` 0.1 as `0.1` — the shortest decimal naming it
/// — and reads that decimal back at the width of the field it lands in: the
/// `f32` 0.1, bit for bit, with nothing reported as adapted.
#[test]
fn a_decimal_in_a_text_save_reads_as_the_f32_it_names() {
    let (src, id) = tenths();
    let record = capture_world(&src).expect("captures");
    let bytes = TextEncoding.encode(&record).expect("text encodes");
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    assert!(
        text.contains("0.1") && !text.contains("0.10000000149011612"),
        "an f32 0.1 is written as `0.1`:\n{text}"
    );

    let mut back = TextEncoding.decode(&bytes).expect("text decodes");
    let range = range_mut(&mut back, id);
    assert!(
        matches!(range, Record::Decimal(_)),
        "a JSON number with a fraction reads as a decimal: {range:?}"
    );

    let mut dst = World::new();
    let applied = apply(&mut dst, &back, Identity::Keep).expect("a decimal reads into an f32");
    assert!(
        applied.report.is_clean(),
        "reading a decimal at the field's width adapts nothing: {:?}",
        applied.report
    );
    let twin = dst.entity_with_id(id).expect("loaded");
    assert_eq!(
        dst.get::<Beacon>(twin).map(|beacon| beacon.range.to_bits()),
        Some(0.1_f32.to_bits())
    );
    let translation = dst.get::<Transform>(twin).expect("a transform").translation;
    assert_eq!(
        [translation.x, translation.y, translation.z].map(f32::to_bits),
        [0.1_f32, 0.2, 0.3].map(f32::to_bits)
    );
}

/// A binary save holds an `f64` as the double it is. The double nearest 0.1
/// is not an `f32`: narrowing it would change the value on load, so it is
/// refused — as written, and through either binary encoding, which keep it a
/// double. The decimal 0.1, by contrast, names an `f32` and reads.
#[test]
fn a_binary_double_is_not_narrowed_into_an_f32() {
    let (src, id) = tenths();
    let mut record = capture_world(&src).expect("captures");
    *range_mut(&mut record, id) = Record::F64(0.1);

    let mut forms = vec![("as recorded", record.clone())];
    for (name, encoding) in [
        ("compact", &CompactEncoding as &dyn SceneEncoding),
        ("msgpack", &MsgPackEncoding),
    ] {
        let mut back = through(&record, name, encoding);
        if name == "compact" {
            assert_eq!(back, record, "the compact encoding keeps a record exactly");
        }
        let range = range_mut(&mut back, id);
        assert!(
            matches!(range, Record::F64(_)),
            "{name}: a binary encoding keeps a double a double: {range:?}"
        );
        forms.push((name, back));
    }
    for (name, form) in forms {
        let mut world = World::new();
        let failure = apply(&mut world, &form, Identity::Keep)
            .err()
            .unwrap_or_else(|| panic!("{name}: the f64 0.1 was narrowed into an f32"));
        assert!(
            failure.message.contains("Beacon"),
            "{name}: the failure names the component: {}",
            failure.message
        );
        assert_eq!(
            world.iter_entities().count(),
            0,
            "{name}: the world changed"
        );
    }

    *range_mut(&mut record, id) = Record::Decimal(0.1);
    let mut world = World::new();
    apply(&mut world, &record, Identity::Keep).expect("the decimal 0.1 is the f32 0.1");
    let twin = world.entity_with_id(id).expect("loaded");
    assert_eq!(
        world
            .get::<Beacon>(twin)
            .map(|beacon| beacon.range.to_bits()),
        Some(0.1_f32.to_bits())
    );
}
