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

//! Implements a sparse bitset for tracking component presence in semantic domains.

/// A simple bitset wrapped around a `Vec<u64>`.
///
/// This structure is used to track which entities have components in a specific
/// `SemanticDomain`. It allows for extremely fast (bitwise) filtering during
/// transversal queries.
#[derive(Debug, Default, Clone)]
pub struct DomainBitset {
    pub(crate) bits: Vec<u64>,
}

impl DomainBitset {
    /// Creates a new, empty bitset.
    pub fn new() -> Self {
        Self { bits: Vec::new() }
    }

    /// Sets the bit at the specified index to 1.
    pub fn set(&mut self, index: u32) {
        let word_idx = (index / 64) as usize;
        let bit_idx = index % 64;

        // Ensure the vector is large enough to hold the bit.
        if word_idx >= self.bits.len() {
            self.bits.resize(word_idx + 1, 0);
        }

        self.bits[word_idx] |= 1 << bit_idx;
    }

    /// Clears the bit at the specified index to 0.
    pub fn clear(&mut self, index: u32) {
        let word_idx = (index / 64) as usize;
        let bit_idx = index % 64;

        if word_idx < self.bits.len() {
            self.bits[word_idx] &= !(1 << bit_idx);
        }
    }

    /// Returns the 64-bit block at the specified word index.
    /// Returns 0 if the index is out of bounds.
    pub fn get_block(&self, word_idx: usize) -> u64 {
        self.bits.get(word_idx).copied().unwrap_or(0)
    }

    /// Returns true if the bit at the specified index is set.
    pub fn is_set(&self, index: u32) -> bool {
        let word_idx = (index / 64) as usize;
        let bit_idx = index % 64;

        if let Some(word) = self.bits.get(word_idx) {
            (word & (1 << bit_idx)) != 0
        } else {
            false
        }
    }

    /// Performs a bitwise AND with another bitset.
    pub fn intersect(&mut self, other: &DomainBitset) {
        let len = self.bits.len().min(other.bits.len());
        for i in 0..len {
            self.bits[i] &= other.bits[i];
        }
        // If other is shorter, the rest of self bits must be cleared.
        if self.bits.len() > len {
            self.bits.truncate(len);
        }
    }
}
