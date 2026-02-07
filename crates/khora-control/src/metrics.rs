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

//! Efficient storage for rolling telemetry metrics.

use khora_core::telemetry::MetricId;
use std::collections::HashMap;

/// A fixed-size circular buffer for storing numerical samples.
#[derive(Debug, Clone)]
pub struct RingBuffer<T, const N: usize> {
    data: [T; N],
    index: usize,
    count: usize,
}

impl<T: Default + Copy, const N: usize> RingBuffer<T, N> {
    /// Creates a new, empty ring buffer.
    pub fn new() -> Self {
        Self {
            data: [T::default(); N],
            index: 0,
            count: 0,
        }
    }

    /// Pushes a new value into the buffer, overwriting the oldest if full.
    pub fn push(&mut self, value: T) {
        self.data[self.index] = value;
        self.index = (self.index + 1) % N;
        if self.count < N {
            self.count += 1;
        }
    }

    /// Returns the number of elements currently in the buffer.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Returns an iterator over the values in chronological order (oldest to newest).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        let (left, right) = self.data.split_at(self.index);
        if self.count < N {
            // Buffer not full: only use values up to the current index
            right[N - self.index..]
                .iter()
                .chain(left[..self.index].iter())
        } else {
            // Buffer full: start from the current index (the oldest value)
            right.iter().chain(left.iter())
        }
    }
}

impl<const N: usize> RingBuffer<f32, N> {
    /// Calculates the arithmetic mean of the values in the buffer.
    pub fn average(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        self.iter().sum::<f32>() / self.count as f32
    }

    /// Calculates the trend (slope) based on a simple linear regression or
    /// just the difference between first and last half.
    /// Returns positive if increasing, negative if decreasing.
    pub fn trend(&self) -> f32 {
        if self.count < 2 {
            return 0.0;
        }
        let half = self.count / 2;
        let first_half_avg: f32 = self.iter().take(half).sum::<f32>() / half as f32;
        let last_half_avg: f32 = self.iter().skip(self.count - half).sum::<f32>() / half as f32;
        last_half_avg - first_half_avg
    }
}

/// Central store for all incoming metrics, organized by ID.
#[derive(Debug, Default)]
pub struct MetricStore {
    // For now we use a simple HashMap.
    // In the future, we might want to use a more dense representation if many metrics exist.
    buffers: HashMap<MetricId, RingBuffer<f32, 120>>, // Stores last 120 samples (e.g. 2s at 60Hz)
}

impl MetricStore {
    /// Creates a new empty metric store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes a new sample for the given metric.
    pub fn push(&mut self, id: MetricId, value: f32) {
        self.buffers
            .entry(id)
            .or_insert_with(RingBuffer::new)
            .push(value);
    }

    /// Returns the average value for a metric, or 0.0 if not found.
    pub fn get_average(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.average()).unwrap_or(0.0)
    }

    /// Returns the trend for a metric, or 0.0 if not found.
    pub fn get_trend(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.trend()).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_push_and_iter() {
        let mut rb = RingBuffer::<f32, 3>::new();
        rb.push(1.0);
        rb.push(2.0);
        rb.push(3.0);
        rb.push(4.0); // Overwrites 1.0

        let values: Vec<f32> = rb.iter().copied().collect();
        assert_eq!(values, vec![2.0, 3.0, 4.0]);
        assert_eq!(rb.count(), 3);
    }

    #[test]
    fn test_ring_buffer_average() {
        let mut rb = RingBuffer::<f32, 4>::new();
        rb.push(10.0);
        rb.push(20.0);
        assert_eq!(rb.average(), 15.0);
    }

    #[test]
    fn test_ring_buffer_trend() {
        let mut rb = RingBuffer::<f32, 4>::new();
        rb.push(1.0);
        rb.push(1.1);
        rb.push(2.0);
        rb.push(2.1);
        // first half: (1.0 + 1.1) / 2 = 1.05
        // second half: (2.0 + 2.1) / 2 = 2.05
        // trend: 2.05 - 1.05 = 1.0
        assert!((rb.trend() - 1.0).abs() < 0.001);
    }
}
