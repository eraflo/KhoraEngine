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

//! The journal behind an asset store's arrivals: the changes not every reader
//! has read yet, and where each reader stands.

use std::collections::{HashMap, VecDeque};

use crate::assets::{Arrival, ArrivalsRead};

/// The arrivals of one asset store that some reader has not read, and each
/// reader's cursor.
///
/// Arrivals are numbered in the order they happened; a cursor is the number
/// of the next arrival its reader has not read. Past `kept` entries the
/// oldest is dropped whatever the readers: a reader it was waiting for then
/// reads a resync, and holds nothing more. An entry is held only while a
/// reader that can still read it has not: with no such reader, nothing is.
#[derive(Debug)]
pub(crate) struct Journal {
    /// The most entries held.
    kept: usize,
    /// The number of `entries[0]`.
    first: u64,
    /// The arrivals held, oldest first.
    entries: VecDeque<Arrival>,
    /// Each reader's cursor.
    readers: HashMap<&'static str, u64>,
}

impl Journal {
    /// Creates an empty journal holding at most `kept` entries, with no reader.
    pub(crate) fn new(kept: usize) -> Self {
        Self {
            kept,
            first: 0,
            entries: VecDeque::new(),
            readers: HashMap::new(),
        }
    }

    /// Returns the most entries this journal holds.
    pub(crate) fn kept(&self) -> usize {
        self.kept
    }

    /// Records `arrival`, dropping the oldest entry once more than `kept` are held.
    pub(crate) fn record(&mut self, arrival: Arrival) {
        self.entries.push_back(arrival);
        while self.entries.len() > self.kept {
            self.entries.pop_front();
            self.first += 1;
        }
        self.drop_read();
    }

    /// Reads what `reader` has not read, moves its cursor past the latest
    /// arrival, and drops what every reader has now read.
    pub(crate) fn read(&mut self, reader: &'static str) -> ArrivalsRead {
        let end = self.first + self.entries.len() as u64;
        let read = match self.readers.get(reader) {
            Some(&cursor) if cursor >= self.first => {
                // `cursor` lies within `first..=end`, so the offset fits the deque.
                let from = (cursor - self.first) as usize;
                ArrivalsRead::Since(self.entries.range(from..).copied().collect())
            }
            _ => ArrivalsRead::Resync,
        };
        self.readers.insert(reader, end);
        self.drop_read();
        read
    }

    /// Returns the number of entries held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Drops the entries no reader still has to read.
    ///
    /// A reader whose cursor fell before the first entry reads a resync, so it
    /// holds nothing; with no other reader, every entry goes.
    fn drop_read(&mut self) {
        let first = self.first;
        let slowest = self
            .readers
            .values()
            .copied()
            .filter(|&cursor| cursor >= first)
            .min()
            .unwrap_or(first + self.entries.len() as u64);
        while self.first < slowest && self.entries.pop_front().is_some() {
            self.first += 1;
        }
    }
}
