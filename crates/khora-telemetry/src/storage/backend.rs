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

//! Storage backends for telemetry data.

use khora_core::telemetry::{
    metrics::MetricType, Metric, MetricId, MetricValue, MetricsError, MetricsResult,
};
use std::fmt::Debug;

pub use super::backend_stats::BackendStats;

/// Trait defining the interface for metrics storage backends
pub trait MetricsBackend: Send + Sync + Debug + 'static {
    /// Returns a reference to this object as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
    /// Stores or updates a metric
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::StorageError`] when the storage cannot be written.
    fn put_metric(&self, metric: Metric) -> MetricsResult<()>;

    /// Returns a metric by ID
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::MetricNotFound`] when no metric has `id`, or
    /// [`MetricsError::StorageError`] when the storage cannot be read.
    fn get_metric(&self, id: &MetricId) -> MetricsResult<Metric>;

    /// Returns whether a metric exists
    fn contains_metric(&self, id: &MetricId) -> bool;

    /// Removes a metric
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::MetricNotFound`] when no metric has `id`, or
    /// [`MetricsError::StorageError`] when the storage cannot be written.
    fn remove_metric(&self, id: &MetricId) -> MetricsResult<()>;

    /// Returns all metric IDs currently stored
    fn list_metric_ids(&self) -> Vec<MetricId>;

    /// Returns all metrics (potentially expensive operation)
    fn list_all_metrics(&self) -> Vec<Metric>;

    /// Clears all metrics
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::StorageError`] when the storage cannot be written.
    fn clear_all(&self) -> MetricsResult<()>;

    /// Returns the number of metrics stored
    fn metric_count(&self) -> usize;

    // Convenience methods for common operations

    /// Increments a counter by the given amount
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::TypeMismatch`] when the metric is not a counter,
    /// or what [`get_metric`](Self::get_metric) and [`put_metric`](Self::put_metric)
    /// return on failure.
    fn increment_counter(&self, id: &MetricId, delta: u64) -> MetricsResult<u64> {
        let mut metric = self.get_metric(id)?;

        match metric.value {
            MetricValue::Counter(ref mut value) => {
                *value = value.saturating_add(delta);
                metric.metadata.update_timestamp();
                let result = *value;
                self.put_metric(metric)?;
                Ok(result)
            }
            _ => Err(MetricsError::TypeMismatch {
                expected: MetricType::Counter,
                found: metric.value.metric_type(),
            }),
        }
    }

    /// Sets a gauge's value
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::TypeMismatch`] when the metric is not a gauge,
    /// or what [`get_metric`](Self::get_metric) and [`put_metric`](Self::put_metric)
    /// return on failure.
    fn set_gauge(&self, id: &MetricId, value: f64) -> MetricsResult<()> {
        let mut metric = self.get_metric(id)?;

        match metric.value {
            MetricValue::Gauge(ref mut gauge_value) => {
                *gauge_value = value;
                metric.metadata.update_timestamp();
                self.put_metric(metric)?;
                Ok(())
            }
            _ => Err(MetricsError::TypeMismatch {
                expected: MetricType::Gauge,
                found: metric.value.metric_type(),
            }),
        }
    }

    /// Adds a sample to a histogram
    ///
    /// # Errors
    ///
    /// Returns [`MetricsError::TypeMismatch`] when the metric is not a histogram,
    /// or what [`get_metric`](Self::get_metric) and [`put_metric`](Self::put_metric)
    /// return on failure.
    fn record_histogram_sample(&self, id: &MetricId, sample: f64) -> MetricsResult<()> {
        let mut metric = self.get_metric(id)?;

        match metric.value {
            MetricValue::Histogram {
                ref mut samples,
                ref bucket_bounds,
                ref mut bucket_counts,
            } => {
                // Add sample to the list
                samples.push(sample);

                // Update bucket counts
                for (i, &bound) in bucket_bounds.iter().enumerate() {
                    if sample <= bound {
                        bucket_counts[i] += 1;
                    }
                }

                metric.metadata.update_timestamp();
                self.put_metric(metric)?;
                Ok(())
            }
            _ => Err(MetricsError::TypeMismatch {
                expected: MetricType::Histogram,
                found: metric.value.metric_type(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::telemetry::metrics::{Metric, MetricId};

    // Mock backend for testing
    #[derive(Debug)]
    struct MockBackend;

    impl MetricsBackend for MockBackend {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn put_metric(&self, _metric: Metric) -> MetricsResult<()> {
            Ok(())
        }

        fn get_metric(&self, id: &MetricId) -> MetricsResult<Metric> {
            Err(MetricsError::MetricNotFound(id.clone()))
        }

        fn contains_metric(&self, _id: &MetricId) -> bool {
            false
        }

        fn remove_metric(&self, id: &MetricId) -> MetricsResult<()> {
            Err(MetricsError::MetricNotFound(id.clone()))
        }

        fn list_metric_ids(&self) -> Vec<MetricId> {
            Vec::new()
        }

        fn list_all_metrics(&self) -> Vec<Metric> {
            Vec::new()
        }

        fn clear_all(&self) -> MetricsResult<()> {
            Ok(())
        }

        fn metric_count(&self) -> usize {
            0
        }
    }

    #[test]
    fn test_backend_trait_compilation() {
        let backend = MockBackend;
        assert_eq!(backend.metric_count(), 0);
        assert!(!backend.contains_metric(&MetricId::new("test", "metric")));
    }
}
