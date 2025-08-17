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

use crate::storage::backend::{BackendStats, MetricsBackend};
use khora_core::telemetry::metrics::{Metric, MetricId, MetricType, MetricsError, MetricsResult};
use std::collections::HashMap;
use std::sync::RwLock;

/// High-performance in-memory metrics backend using RwLock<HashMap>
///
/// This implementation provides:
/// - Thread-safe concurrent access (multiple readers, single writer)
/// - O(1) average case lookup and insertion
/// - Memory-efficient storage
/// - Lock-free reads when possible
#[derive(Debug)]
pub struct InMemoryBackend {
    /// The core storage - RwLock allows concurrent reads
    storage: RwLock<HashMap<MetricId, Metric>>,
}

impl InMemoryBackend {
    /// Create a new in-memory backend
    pub fn new() -> Self {
        Self {
            storage: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new in-memory backend with initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            storage: RwLock::new(HashMap::with_capacity(capacity)),
        }
    }

    /// Get statistics about this backend
    pub fn get_stats(&self) -> BackendStats {
        let storage = self.storage.read().unwrap();

        let mut counter_count = 0;
        let mut gauge_count = 0;
        let mut histogram_count = 0;

        for metric in storage.values() {
            match metric.value.metric_type() {
                MetricType::Counter => counter_count += 1,
                MetricType::Gauge => gauge_count += 1,
                MetricType::Histogram => histogram_count += 1,
            }
        }

        // Rough estimate of memory usage
        let estimated_memory_bytes = storage.len() * std::mem::size_of::<(MetricId, Metric)>()
            + storage.capacity() * std::mem::size_of::<(MetricId, Metric)>();

        BackendStats {
            total_metrics: storage.len(),
            counter_count,
            gauge_count,
            histogram_count,
            estimated_memory_bytes,
        }
    }

    /// Get metrics by namespace
    pub fn get_metrics_by_namespace(&self, namespace: &str) -> Vec<Metric> {
        let storage = self.storage.read().unwrap();
        storage
            .values()
            .filter(|metric| metric.metadata.id.namespace == namespace)
            .cloned()
            .collect()
    }

    /// Get metrics by type
    pub fn get_metrics_by_type(&self, metric_type: MetricType) -> Vec<Metric> {
        let storage = self.storage.read().unwrap();
        storage
            .values()
            .filter(|metric| metric.metadata.metric_type == metric_type)
            .cloned()
            .collect()
    }

    /// Bulk insert metrics (more efficient than individual puts)
    pub fn put_metrics(&self, metrics: Vec<Metric>) -> MetricsResult<()> {
        let mut storage = self
            .storage
            .write()
            .map_err(|_| MetricsError::StorageError("Failed to acquire write lock".to_string()))?;

        for metric in metrics {
            storage.insert(metric.metadata.id.clone(), metric);
        }

        Ok(())
    }

    /// Remove metrics by namespace
    pub fn remove_metrics_by_namespace(&self, namespace: &str) -> MetricsResult<usize> {
        let mut storage = self
            .storage
            .write()
            .map_err(|_| MetricsError::StorageError("Failed to acquire write lock".to_string()))?;

        let to_remove: Vec<_> = storage
            .keys()
            .filter(|id| id.namespace == namespace)
            .cloned()
            .collect();

        let removed_count = to_remove.len();
        for id in to_remove {
            storage.remove(&id);
        }

        Ok(removed_count)
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsBackend for InMemoryBackend {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn put_metric(&self, metric: Metric) -> MetricsResult<()> {
        let mut storage = self
            .storage
            .write()
            .map_err(|_| MetricsError::StorageError("Failed to acquire write lock".to_string()))?;

        storage.insert(metric.metadata.id.clone(), metric);
        Ok(())
    }

    fn get_metric(&self, id: &MetricId) -> MetricsResult<Metric> {
        let storage = self
            .storage
            .read()
            .map_err(|_| MetricsError::StorageError("Failed to acquire read lock".to_string()))?;

        storage
            .get(id)
            .cloned()
            .ok_or_else(|| MetricsError::MetricNotFound(id.clone()))
    }

    fn contains_metric(&self, id: &MetricId) -> bool {
        if let Ok(storage) = self.storage.read() {
            storage.contains_key(id)
        } else {
            false
        }
    }

    fn remove_metric(&self, id: &MetricId) -> MetricsResult<()> {
        let mut storage = self
            .storage
            .write()
            .map_err(|_| MetricsError::StorageError("Failed to acquire write lock".to_string()))?;

        storage
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| MetricsError::MetricNotFound(id.clone()))
    }

    fn list_metric_ids(&self) -> Vec<MetricId> {
        if let Ok(storage) = self.storage.read() {
            storage.keys().cloned().collect()
        } else {
            Vec::new()
        }
    }

    fn list_all_metrics(&self) -> Vec<Metric> {
        if let Ok(storage) = self.storage.read() {
            storage.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    fn clear_all(&self) -> MetricsResult<()> {
        let mut storage = self
            .storage
            .write()
            .map_err(|_| MetricsError::StorageError("Failed to acquire write lock".to_string()))?;

        storage.clear();
        Ok(())
    }

    fn metric_count(&self) -> usize {
        if let Ok(storage) = self.storage.read() {
            storage.len()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::telemetry::metrics::{Metric, MetricId, MetricValue};

    #[test]
    fn test_in_memory_backend_basic_operations() {
        let backend = InMemoryBackend::new();
        let id = MetricId::new("test", "counter");
        let metric = Metric::new_counter(id.clone(), "Test counter", 42);

        // Test put and get
        assert!(backend.put_metric(metric.clone()).is_ok());
        assert!(backend.contains_metric(&id));

        let retrieved = backend.get_metric(&id).unwrap();
        assert_eq!(retrieved.value.as_counter(), Some(42));
        assert_eq!(backend.metric_count(), 1);

        // Test remove
        assert!(backend.remove_metric(&id).is_ok());
        assert!(!backend.contains_metric(&id));
        assert_eq!(backend.metric_count(), 0);
    }

    #[test]
    fn test_counter_increment() {
        let backend = InMemoryBackend::new();
        let id = MetricId::new("test", "counter");
        let metric = Metric::new_counter(id.clone(), "Test counter", 0);

        backend.put_metric(metric).unwrap();

        // Test increment
        let new_value = backend.increment_counter(&id, 5).unwrap();
        assert_eq!(new_value, 5);

        let new_value = backend.increment_counter(&id, 3).unwrap();
        assert_eq!(new_value, 8);

        // Verify the stored value
        let retrieved = backend.get_metric(&id).unwrap();
        assert_eq!(retrieved.value.as_counter(), Some(8));
    }

    #[test]
    fn test_gauge_operations() {
        let backend = InMemoryBackend::new();
        let id = MetricId::new("test", "gauge");
        let metric = Metric::new_gauge(id.clone(), "Test gauge", "bytes", 100.0);

        backend.put_metric(metric).unwrap();

        // Test set gauge
        backend.set_gauge(&id, 250.5).unwrap();

        let retrieved = backend.get_metric(&id).unwrap();
        assert_eq!(retrieved.value.as_gauge(), Some(250.5));
    }

    #[test]
    fn test_histogram_operations() {
        let backend = InMemoryBackend::new();
        let id = MetricId::new("test", "histogram");
        let buckets = vec![1.0, 5.0, 10.0, 50.0, 100.0];
        let metric = Metric::new_histogram(id.clone(), "Test histogram", "ms", buckets);

        backend.put_metric(metric).unwrap();

        // Add some samples
        backend.record_histogram_sample(&id, 0.5).unwrap(); // <= 1.0, 5.0, 10.0, 50.0, 100.0
        backend.record_histogram_sample(&id, 3.0).unwrap(); // <= 5.0, 10.0, 50.0, 100.0 (not <= 1.0)
        backend.record_histogram_sample(&id, 7.0).unwrap(); // <= 10.0, 50.0, 100.0 (not <= 1.0, 5.0)
        backend.record_histogram_sample(&id, 25.0).unwrap(); // <= 50.0, 100.0 (not <= 1.0, 5.0, 10.0)

        let retrieved = backend.get_metric(&id).unwrap();
        if let MetricValue::Histogram {
            samples,
            bucket_counts,
            ..
        } = retrieved.value
        {
            assert_eq!(samples.len(), 4);
            // Cumulative buckets: each bucket counts all samples <= its bound
            assert_eq!(bucket_counts[0], 1); // 0.5 <= 1.0
            assert_eq!(bucket_counts[1], 2); // 0.5, 3.0 <= 5.0
            assert_eq!(bucket_counts[2], 3); // 0.5, 3.0, 7.0 <= 10.0
            assert_eq!(bucket_counts[3], 4); // 0.5, 3.0, 7.0, 25.0 <= 50.0
            assert_eq!(bucket_counts[4], 4); // all samples <= 100.0
        }
    }

    #[test]
    fn test_bulk_operations() {
        let backend = InMemoryBackend::new();

        let metrics = vec![
            Metric::new_counter(MetricId::new("engine", "frame_count"), "Frame counter", 100),
            Metric::new_gauge(
                MetricId::new("memory", "heap_mb"),
                "Heap usage",
                "MB",
                512.0,
            ),
            Metric::new_counter(
                MetricId::new("renderer", "triangles"),
                "Triangle counter",
                50000,
            ),
        ];

        backend.put_metrics(metrics).unwrap();
        assert_eq!(backend.metric_count(), 3);

        // Test namespace filtering
        let engine_metrics = backend.get_metrics_by_namespace("engine");
        assert_eq!(engine_metrics.len(), 1);
        assert_eq!(engine_metrics[0].metadata.id.name, "frame_count");

        // Test type filtering
        let counters = backend.get_metrics_by_type(MetricType::Counter);
        assert_eq!(counters.len(), 2);

        // Test namespace removal
        let removed = backend.remove_metrics_by_namespace("engine").unwrap();
        assert_eq!(removed, 1);
        assert_eq!(backend.metric_count(), 2);
    }

    #[test]
    fn test_backend_stats() {
        let backend = InMemoryBackend::new();

        backend
            .put_metric(Metric::new_counter(
                MetricId::new("test", "c1"),
                "Counter 1",
                0,
            ))
            .unwrap();
        backend
            .put_metric(Metric::new_counter(
                MetricId::new("test", "c2"),
                "Counter 2",
                0,
            ))
            .unwrap();
        backend
            .put_metric(Metric::new_gauge(
                MetricId::new("test", "g1"),
                "Gauge 1",
                "unit",
                0.0,
            ))
            .unwrap();

        let stats = backend.get_stats();
        assert_eq!(stats.total_metrics, 3);
        assert_eq!(stats.counter_count, 2);
        assert_eq!(stats.gauge_count, 1);
        assert_eq!(stats.histogram_count, 0);
        assert!(stats.estimated_memory_bytes > 0);
    }

    #[test]
    fn test_type_mismatch_errors() {
        let backend = InMemoryBackend::new();
        let id = MetricId::new("test", "gauge");
        let metric = Metric::new_gauge(id.clone(), "Test gauge", "bytes", 100.0);

        backend.put_metric(metric).unwrap();

        // Try to increment a gauge as if it were a counter
        let result = backend.increment_counter(&id, 5);
        assert!(result.is_err());
        if let Err(MetricsError::TypeMismatch { expected, found }) = result {
            assert_eq!(expected, MetricType::Counter);
            assert_eq!(found, MetricType::Gauge);
        }
    }

    #[test]
    fn test_not_found_errors() {
        let backend = InMemoryBackend::new();
        let id = MetricId::new("test", "nonexistent");

        let result = backend.get_metric(&id);
        assert!(result.is_err());
        if let Err(MetricsError::MetricNotFound(missing_id)) = result {
            assert_eq!(missing_id, id);
        }
    }
}
