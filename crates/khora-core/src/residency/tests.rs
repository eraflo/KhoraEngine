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

/// A CPU asset: a number.
struct Number(u32);
impl Asset for Number {}

/// A device that counts what it made and what it was given back.
#[derive(Default)]
struct Counter {
    made: Vec<u32>,
    released: Vec<u32>,
    refuse: bool,
}

/// Doubles a number "on the device".
struct Doubled;

impl Residency for Doubled {
    const NAME: &'static str = "doubled";
    const DEVICE: DeviceClass = DeviceClass::Cpu;
    type Source = Number;
    type Resident = u32;
    type Target<'a> = &'a mut Counter;

    fn make(source: &Number, target: &mut &mut Counter) -> Result<u32, ResidencyError> {
        if target.refuse {
            return Err(ResidencyError {
                residency: Self::NAME,
                reason: "refused".to_owned(),
            });
        }
        target.made.push(source.0);
        Ok(source.0 * 2)
    }

    fn release(resident: u32, target: &mut &mut Counter) {
        target.released.push(resident);
    }
}

fn key() -> ResidentKey {
    AssetUUID::new_v5("number")
}

#[test]
fn the_same_source_copy_is_made_once() {
    let store = Resident::<Doubled>::new();
    let mut counter = Counter::default();
    let source = AssetHandle::new(Number(3));
    for _ in 0..3 {
        let resident = store
            .ensure(key(), &source, &mut &mut counter)
            .expect("made");
        assert_eq!(resident, 6);
    }
    assert_eq!(counter.made, vec![3], "made once");
    assert_eq!(store.get(&key()), Some(6));
}

#[test]
fn a_replaced_source_is_made_again_and_the_stale_resident_released() {
    let store = Resident::<Doubled>::new();
    let mut counter = Counter::default();
    store
        .ensure(key(), &AssetHandle::new(Number(3)), &mut &mut counter)
        .expect("made");
    let replaced = AssetHandle::new(Number(5));
    let resident = store
        .ensure(key(), &replaced, &mut &mut counter)
        .expect("made again");
    assert_eq!(resident, 10);
    assert_eq!(counter.made, vec![3, 5]);
    assert_eq!(counter.released, vec![6], "the stale resident goes back");
    assert_eq!(store.len(), 1);
}

#[test]
fn a_failed_make_keeps_the_last_good_resident() {
    let store = Resident::<Doubled>::new();
    let mut counter = Counter::default();
    store
        .ensure(key(), &AssetHandle::new(Number(3)), &mut &mut counter)
        .expect("made");
    counter.refuse = true;
    let refused = store.ensure(key(), &AssetHandle::new(Number(5)), &mut &mut counter);
    assert!(refused.is_err());
    assert_eq!(store.get(&key()), Some(6), "the last good copy stays");
    assert!(counter.released.is_empty());
}

#[test]
fn a_resident_placed_by_hand_is_made_from_its_source_next() {
    let store = Resident::<Doubled>::new();
    let mut counter = Counter::default();
    store.insert(key(), 100);
    let resident = store
        .ensure(key(), &AssetHandle::new(Number(4)), &mut &mut counter)
        .expect("made");
    assert_eq!(resident, 8);
    assert_eq!(counter.released, vec![100]);
}

#[test]
fn evict_releases_the_resident() {
    let store = Resident::<Doubled>::new();
    let mut counter = Counter::default();
    store
        .ensure(key(), &AssetHandle::new(Number(2)), &mut &mut counter)
        .expect("made");
    store.evict(&key(), &mut &mut counter);
    assert!(store.is_empty());
    assert_eq!(counter.released, vec![4]);
}
