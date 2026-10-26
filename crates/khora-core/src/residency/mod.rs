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

//! Residency — keeping a copy of an asset where a device can use it.
//!
//! A mesh is drawn from vertex buffers on the GPU, a UI image from a rect of
//! the GPU atlas. A **residency** names that move: what it starts from (its
//! [`Source`](Residency::Source), a CPU asset), what it makes (its
//! [`Resident`](Residency::Resident)), and what it makes it on (its
//! [`Target`](Residency::Target): the device, or a pool on it).
//!
//! The data layer runs each residency as a `PreExtract` DataSystem, after the
//! asset reload: a [`Resident`] store remembers which copy of the source each
//! resident was made from, so a source the reload replaced is made again and
//! the stale resident released. Hot reload is the store's job, not each
//! residency's.
//!
//! Only the contract lives here; the residencies and their DataSystems are in
//! `khora-data`, the devices in `khora-infra`.

use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

use crate::asset::{Asset, AssetHandle, AssetUUID};

/// The kind of device a residency makes its residents on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceClass {
    /// The host processor's memory.
    Cpu,
    /// A graphics processor.
    Gpu,
    /// A neural processor.
    Npu,
    /// Any other device a backend brings.
    Other,
}

/// What a resident is kept under: the asset it was made from.
pub type ResidentKey = AssetUUID;

/// Why a residency could not make a resident.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidencyError {
    /// The residency that failed.
    pub residency: &'static str,
    /// What the device refused, in words.
    pub reason: String,
}

impl fmt::Display for ResidencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.residency, self.reason)
    }
}

impl std::error::Error for ResidencyError {}

/// Moving an asset to the device that uses it.
pub trait Residency: Send + Sync + 'static {
    /// The residency's name: its DataSystem's name, as telemetry and
    /// `runs_after` name it.
    const NAME: &'static str;
    /// The kind of device its residents live on.
    const DEVICE: DeviceClass;
    /// The CPU asset a resident is made from.
    type Source: Asset;
    /// What lives on the device: handles to it, cheap to copy out.
    type Resident: Clone + Send + Sync;
    /// What [`make`](Self::make) and [`release`](Self::release) work on for
    /// one frame — the device, or a pool on it such as an atlas.
    type Target<'a>;

    /// Makes the resident of `source` on `target`.
    fn make(
        source: &Self::Source,
        target: &mut Self::Target<'_>,
    ) -> Result<Self::Resident, ResidencyError>;

    /// Gives `resident` back to `target`, once nothing uses it.
    fn release(resident: Self::Resident, target: &mut Self::Target<'_>);
}

/// One resident and the copy of the source it was made from.
struct Entry<R: Residency> {
    /// `None` for a resident placed by hand: no source copy vouches for it,
    /// so [`Resident::ensure`] makes it again from the first source it sees.
    source: Option<AssetHandle<R::Source>>,
    resident: R::Resident,
}

/// The residents of one residency, each keyed by its asset.
///
/// Each entry holds the source copy it was made from — a clone of its
/// handle, so that copy stays alive and no later copy can be mistaken for it.
/// [`ensure`](Self::ensure) compares that copy with the one the asset store
/// holds now: the same copy reuses the resident, another makes it again.
pub struct Resident<R: Residency> {
    entries: RwLock<HashMap<ResidentKey, Entry<R>>>,
}

impl<R: Residency> Default for Resident<R> {
    fn default() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }
}

impl<R: Residency> Resident<R> {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The resident kept under `key`, whatever copy it was made from.
    pub fn get(&self, key: &ResidentKey) -> Option<R::Resident> {
        self.entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .map(|entry| entry.resident.clone())
    }

    /// The resident of `source` under `key`: the one kept, when it was made
    /// from this very copy; otherwise a new one, made on `target`, which
    /// replaces and releases the stale one.
    ///
    /// A failed make keeps the stale resident: the last good copy stays on
    /// the device until the source makes one.
    pub fn ensure(
        &self,
        key: ResidentKey,
        source: &AssetHandle<R::Source>,
        target: &mut R::Target<'_>,
    ) -> Result<R::Resident, ResidencyError> {
        if let Some(entry) = self
            .entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            if entry.source.as_ref() == Some(source) {
                return Ok(entry.resident.clone());
            }
        }
        let resident = R::make(source, target)?;
        let stale = self
            .entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                key,
                Entry {
                    source: Some(source.clone()),
                    resident: resident.clone(),
                },
            );
        if let Some(stale) = stale {
            R::release(stale.resident, target);
        }
        Ok(resident)
    }

    /// Keeps `resident` under `key` with no source copy behind it. The next
    /// [`ensure`](Self::ensure) of `key` makes it again from its source.
    pub fn insert(&self, key: ResidentKey, resident: R::Resident) {
        self.entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                key,
                Entry {
                    source: None,
                    resident,
                },
            );
    }

    /// Removes the resident under `key` and releases it on `target`.
    pub fn evict(&self, key: &ResidentKey, target: &mut R::Target<'_>) {
        let removed = self
            .entries
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(key);
        if let Some(entry) = removed {
            R::release(entry.resident, target);
        }
    }

    /// The number of residents kept.
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Whether no resident is kept.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
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
}
