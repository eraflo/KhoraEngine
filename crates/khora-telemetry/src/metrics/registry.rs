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

//! Registry for managing metrics.

use crate::storage::{backend::MetricsBackend, memory_backend::InMemoryBackend};
use khora_core::telemetry::metrics::{Metric, MetricId, MetricType, MetricsError, MetricsResult};
use std::sync::Arc;

/// Central registry for metrics in the KhoraEngine
///
/// This registry provides a high-level API for metrics management and
/// serves as the main entry point for the metrics system. It handles
/// metric registration, updates, and queries while providing type safety
/// and convenient helper methods.
#[derive(Debug)]
pub struct MetricsRegistry {
    backend: Arc<dyn MetricsBackend>,
}

impl MetricsRegistry {
    /// Create a new metrics registry with the default in-memory backend
    pub fn new() -> Self {
        Self {
            backend: Arc::new(InMemoryBackend::new()),
        }
    }

    /// Create a new metrics registry with a custom backend
    pub fn with_backend(backend: Arc<dyn MetricsBackend>) -> Self {
        Self { backend }
    }

    /// Register a new counter metric
    pub fn register_counter(
        &self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> MetricsResult<CounterHandle> {
        let id = MetricId::new(namespace, name);
        let metric = Metric::new_counter(id.clone(), description, 0);
        self.backend.put_metric(metric)?;
        Ok(CounterHandle::new(id, self.backend.clone()))
    }

    /// Register a new counter metric with labels
    pub fn register_counter_with_labels(
        &self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        labels: Vec<(String, String)>,
    ) -> MetricsResult<CounterHandle> {
        let mut id = MetricId::new(namespace, name);
        for (key, value) in labels {
            id = id.with_label(key, value);
        }
        let metric = Metric::new_counter(id.clone(), description, 0);
        self.backend.put_metric(metric)?;
        Ok(CounterHandle::new(id, self.backend.clone()))
    }

    /// Register a new gauge metric
    pub fn register_gauge(
        &self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        unit: impl Into<String>,
    ) -> MetricsResult<GaugeHandle> {
        let id = MetricId::new(namespace, name);
        let metric = Metric::new_gauge(id.clone(), description, unit, 0.0);
        self.backend.put_metric(metric)?;
        Ok(GaugeHandle::new(id, self.backend.clone()))
    }

    /// Register a new gauge metric with labels
    pub fn register_gauge_with_labels(
        &self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        unit: impl Into<String>,
        labels: Vec<(String, String)>,
    ) -> MetricsResult<GaugeHandle> {
        let mut id = MetricId::new(namespace, name);
        for (key, value) in labels {
            id = id.with_label(key, value);
        }
        let metric = Metric::new_gauge(id.clone(), description, unit, 0.0);
        self.backend.put_metric(metric)?;
        Ok(GaugeHandle::new(id, self.backend.clone()))
    }

    /// Register a new histogram metric
    pub fn register_histogram(
        &self,
        namespace: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        unit: impl Into<String>,
        buckets: Vec<f64>,
    ) -> MetricsResult<HistogramHandle> {
        let id = MetricId::new(namespace, name);
        let metric = Metric::new_histogram(id.clone(), description, unit, buckets);
        self.backend.put_metric(metric)?;
        Ok(HistogramHandle::new(id, self.backend.clone()))
    }

    /// Get a metric by ID
    pub fn get_metric(&self, id: &MetricId) -> MetricsResult<Metric> {
        self.backend.get_metric(id)
    }

    /// Check if a metric exists
    pub fn contains_metric(&self, id: &MetricId) -> bool {
        self.backend.contains_metric(id)
    }

    /// Get all metrics in a namespace
    pub fn get_namespace_metrics(&self, namespace: &str) -> Vec<Metric> {
        // Try to cast to InMemoryBackend for more efficient operation
        if let Some(memory_backend) = self
            .backend
            .as_ref()
            .as_any()
            .downcast_ref::<InMemoryBackend>()
        {
            memory_backend.get_metrics_by_namespace(namespace)
        } else {
            // Fallback for other backends
            self.backend
                .list_all_metrics()
                .into_iter()
                .filter(|m| m.metadata.id.namespace == namespace)
                .collect()
        }
    }

    /// Get all counters
    pub fn get_all_counters(&self) -> Vec<Metric> {
        if let Some(memory_backend) = self
            .backend
            .as_ref()
            .as_any()
            .downcast_ref::<InMemoryBackend>()
        {
            memory_backend.get_metrics_by_type(MetricType::Counter)
        } else {
            self.backend
                .list_all_metrics()
                .into_iter()
                .filter(|m| m.metadata.metric_type == MetricType::Counter)
                .collect()
        }
    }

    /// Get all gauges
    pub fn get_all_gauges(&self) -> Vec<Metric> {
        if let Some(memory_backend) = self
            .backend
            .as_ref()
            .as_any()
            .downcast_ref::<InMemoryBackend>()
        {
            memory_backend.get_metrics_by_type(MetricType::Gauge)
        } else {
            self.backend
                .list_all_metrics()
                .into_iter()
                .filter(|m| m.metadata.metric_type == MetricType::Gauge)
                .collect()
        }
    }

    /// Get the total number of metrics
    pub fn metric_count(&self) -> usize {
        self.backend.metric_count()
    }

    /// Clear all metrics
    pub fn clear_all(&self) -> MetricsResult<()> {
        self.backend.clear_all()
    }

    /// Get direct access to the backend (for advanced operations)
    pub fn backend(&self) -> &Arc<dyn MetricsBackend> {
        &self.backend
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for efficient counter operations
#[derive(Debug, Clone)]
pub struct CounterHandle {
    id: MetricId,
    backend: Arc<dyn MetricsBackend>,
}

impl CounterHandle {
    fn new(id: MetricId, backend: Arc<dyn MetricsBackend>) -> Self {
        Self { id, backend }
    }

    /// Increment the counter by 1
    pub fn increment(&self) -> MetricsResult<u64> {
        self.backend.increment_counter(&self.id, 1)
    }

    /// Increment the counter by a specific amount
    pub fn increment_by(&self, amount: u64) -> MetricsResult<u64> {
        self.backend.increment_counter(&self.id, amount)
    }

    /// Get the current counter value
    pub fn get(&self) -> MetricsResult<u64> {
        let metric = self.backend.get_metric(&self.id)?;
        metric
            .value
            .as_counter()
            .ok_or_else(|| MetricsError::TypeMismatch {
                expected: MetricType::Counter,
                found: metric.value.metric_type(),
            })
    }

    /// Get the metric ID
    pub fn id(&self) -> &MetricId {
        &self.id
    }
}

/// Handle for efficient gauge operations
#[derive(Debug, Clone)]
pub struct GaugeHandle {
    id: MetricId,
    backend: Arc<dyn MetricsBackend>,
}

impl GaugeHandle {
    fn new(id: MetricId, backend: Arc<dyn MetricsBackend>) -> Self {
        Self { id, backend }
    }

    /// Set the gauge to a specific value
    pub fn set(&self, value: f64) -> MetricsResult<()> {
        self.backend.set_gauge(&self.id, value)
    }

    /// Increment the gauge by a specific amount
    pub fn add(&self, delta: f64) -> MetricsResult<f64> {
        let current = self.get()?;
        let new_value = current + delta;
        self.set(new_value)?;
        Ok(new_value)
    }

    /// Decrement the gauge by a specific amount
    pub fn sub(&self, delta: f64) -> MetricsResult<f64> {
        self.add(-delta)
    }

    /// Get the current gauge value
    pub fn get(&self) -> MetricsResult<f64> {
        let metric = self.backend.get_metric(&self.id)?;
        metric
            .value
            .as_gauge()
            .ok_or_else(|| MetricsError::TypeMismatch {
                expected: MetricType::Gauge,
                found: metric.value.metric_type(),
            })
    }

    /// Get the metric ID
    pub fn id(&self) -> &MetricId {
        &self.id
    }
}

/// Handle for efficient histogram operations
#[derive(Debug, Clone)]
pub struct HistogramHandle {
    id: MetricId,
    backend: Arc<dyn MetricsBackend>,
}

impl HistogramHandle {
    fn new(id: MetricId, backend: Arc<dyn MetricsBackend>) -> Self {
        Self { id, backend }
    }

    /// Record a sample in the histogram
    pub fn observe(&self, value: f64) -> MetricsResult<()> {
        self.backend.record_histogram_sample(&self.id, value)
    }

    /// Get the metric ID
    pub fn id(&self) -> &MetricId {
        &self.id
    }

    /// Get the full histogram metric (for analysis)
    pub fn get_metric(&self) -> MetricsResult<Metric> {
        self.backend.get_metric(&self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = MetricsRegistry::new();
        assert_eq!(registry.metric_count(), 0);
    }

    #[test]
    fn test_counter_registration_and_operations() {
        let registry = MetricsRegistry::new();

        let counter = registry
            .register_counter("engine", "frame_count", "Total number of frames rendered")
            .unwrap();

        // Test increment operations
        assert_eq!(counter.increment().unwrap(), 1);
        assert_eq!(counter.increment_by(5).unwrap(), 6);
        assert_eq!(counter.get().unwrap(), 6);

        // Verify it's stored in the registry
        assert!(registry.contains_metric(counter.id()));
        assert_eq!(registry.metric_count(), 1);
    }

    #[test]
    fn test_gauge_registration_and_operations() {
        let registry = MetricsRegistry::new();

        let gauge = registry
            .register_gauge("memory", "heap_usage", "Current heap usage", "MB")
            .unwrap();

        // Test gauge operations
        gauge.set(100.5).unwrap();
        assert_eq!(gauge.get().unwrap(), 100.5);

        assert_eq!(gauge.add(50.0).unwrap(), 150.5);
        assert_eq!(gauge.sub(25.0).unwrap(), 125.5);

        // Verify it's stored in the registry
        assert!(registry.contains_metric(gauge.id()));
    }

    #[test]
    fn test_histogram_registration_and_operations() {
        let registry = MetricsRegistry::new();

        let histogram = registry
            .register_histogram(
                "renderer",
                "frame_time",
                "Frame rendering time distribution",
                "ms",
                vec![1.0, 5.0, 10.0, 50.0, 100.0],
            )
            .unwrap();

        // Test histogram operations
        histogram.observe(2.5).unwrap();
        histogram.observe(15.0).unwrap();
        histogram.observe(75.0).unwrap();

        // Verify it's stored
        assert!(registry.contains_metric(histogram.id()));

        let metric = histogram.get_metric().unwrap();
        if let khora_core::telemetry::metrics::MetricValue::Histogram { samples, .. } = metric.value
        {
            assert_eq!(samples.len(), 3);
            assert!(samples.contains(&2.5));
            assert!(samples.contains(&15.0));
            assert!(samples.contains(&75.0));
        } else {
            panic!("Expected histogram metric");
        }
    }

    #[test]
    fn test_metrics_with_labels() {
        let registry = MetricsRegistry::new();

        let counter = registry
            .register_counter_with_labels(
                "renderer",
                "triangles_rendered",
                "Number of triangles rendered",
                vec![
                    ("quality".to_string(), "high".to_string()),
                    ("pass".to_string(), "main".to_string()),
                ],
            )
            .unwrap();

        counter.increment_by(1000).unwrap();

        let id_str = counter.id().to_string_formatted();
        assert!(id_str.contains("quality=high"));
        assert!(id_str.contains("pass=main"));
    }

    #[test]
    fn test_namespace_filtering() {
        let registry = MetricsRegistry::new();

        registry
            .register_counter("engine", "frames", "Frame count")
            .unwrap();
        registry
            .register_counter("engine", "updates", "Update count")
            .unwrap();
        registry
            .register_counter("renderer", "draws", "Draw calls")
            .unwrap();
        registry
            .register_gauge("memory", "heap", "Heap usage", "MB")
            .unwrap();

        let engine_metrics = registry.get_namespace_metrics("engine");
        assert_eq!(engine_metrics.len(), 2);

        let renderer_metrics = registry.get_namespace_metrics("renderer");
        assert_eq!(renderer_metrics.len(), 1);

        let memory_metrics = registry.get_namespace_metrics("memory");
        assert_eq!(memory_metrics.len(), 1);
    }

    #[test]
    fn test_type_filtering() {
        let registry = MetricsRegistry::new();

        registry
            .register_counter("test", "c1", "Counter 1")
            .unwrap();
        registry
            .register_counter("test", "c2", "Counter 2")
            .unwrap();
        registry
            .register_gauge("test", "g1", "Gauge 1", "unit")
            .unwrap();

        let counters = registry.get_all_counters();
        assert_eq!(counters.len(), 2);

        let gauges = registry.get_all_gauges();
        assert_eq!(gauges.len(), 1);
    }

    #[test]
    fn test_clear_all() {
        let registry = MetricsRegistry::new();

        registry
            .register_counter("test", "counter", "Test counter")
            .unwrap();
        registry
            .register_gauge("test", "gauge", "Test gauge", "unit")
            .unwrap();

        assert_eq!(registry.metric_count(), 2);

        registry.clear_all().unwrap();
        assert_eq!(registry.metric_count(), 0);
    }
}
