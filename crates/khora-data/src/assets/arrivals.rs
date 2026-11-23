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

//! The arrivals journal of an asset store: which copies landed, were replaced or went.

use khora_core::asset::AssetUUID;

/// The most entries one store's arrivals journal keeps.
pub const ARRIVALS_KEPT: usize = 4096;

/// What happened to the copy held under a uuid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrivalKind {
    /// A copy was inserted under a uuid the store did not hold.
    Landed,
    /// A copy was inserted under a uuid the store already held.
    Replaced,
    /// The copy held under the uuid was removed.
    Gone,
}

/// One change to an asset store: the uuid it touched and what happened to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrival {
    /// The uuid whose copy changed.
    pub uuid: AssetUUID,
    /// What happened to that copy.
    pub kind: ArrivalKind,
}

/// What one reader of an arrivals journal gets back from a read.
#[derive(Debug, PartialEq)]
pub enum ArrivalsRead {
    /// The arrivals since this reader's last read, in the order they happened.
    Since(Vec<Arrival>),
    /// The reader is new or fell behind the journal: it must read the whole store.
    ///
    /// Its cursor now stands at the latest arrival.
    Resync,
}
