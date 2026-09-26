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

mod ring_buffer;

pub use ring_buffer::RingBuffer;

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
        self.buffers.entry(id).or_default().push(value);
    }

    /// Returns the average value for a metric, or 0.0 if not found.
    pub fn get_average(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.average()).unwrap_or(0.0)
    }

    /// Returns the trend for a metric, or 0.0 if not found.
    pub fn get_trend(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.trend()).unwrap_or(0.0)
    }

    /// Returns the variance for a metric, or 0.0 if not found.
    ///
    /// High variance in frame times is a strong stutter indicator.
    pub fn get_variance(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.variance()).unwrap_or(0.0)
    }

    /// Returns the maximum value for a metric, or 0.0 if not found.
    pub fn get_max(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.max()).unwrap_or(f32::MIN)
    }

    /// Returns the minimum value for a metric, or 0.0 if not found.
    pub fn get_min(&self, id: &MetricId) -> f32 {
        self.buffers.get(id).map(|b| b.min()).unwrap_or(f32::MAX)
    }

    /// Returns the sample count for a metric, or 0 if not found.
    pub fn get_sample_count(&self, id: &MetricId) -> usize {
        self.buffers.get(id).map(|b| b.count()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_store_variance_and_extremes() {
        let mut store = MetricStore::new();
        let id = MetricId::new("test", "values");
        store.push(id.clone(), 5.0);
        store.push(id.clone(), 15.0);
        store.push(id.clone(), 5.0);
        store.push(id.clone(), 15.0);

        assert!((store.get_variance(&id) - 25.0).abs() < 0.01);
        assert_eq!(store.get_min(&id), 5.0);
        assert_eq!(store.get_max(&id), 15.0);
        assert_eq!(store.get_sample_count(&id), 4);
    }
}
