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

//! Abstract definitions for engine metrics and telemetry.

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::time::Instant;

/// A unique, structured identifier for a metric.
///
/// A `MetricId` is composed of a namespace, a name, and a set of key-value labels,
/// allowing for powerful filtering and querying of telemetry data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricId {
    /// The broad category of the metric (e.g., "renderer", "memory").
    pub namespace: String,
    /// The specific name of the metric (e.g., "frame_time_ms", "triangles_rendered").
    pub name: String,
    /// Optional, sorted key-value pairs for dimensional filtering.
    pub labels: Vec<(String, String)>,
}

impl MetricId {
    /// Creates a new `MetricId` with a namespace and a name.
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            labels: Vec::new(),
        }
    }

    /// Adds a dimensional label to the metric ID, returning a new `MetricId`.
    /// Labels are kept sorted by key for consistent hashing and display.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        self.labels.sort_by(|a, b| a.0.cmp(&b.0));
        self
    }

    /// Returns a formatted string representation of the ID (e.g., "namespace:name[k=v,...]").
    pub fn to_string_formatted(&self) -> String {
        if self.labels.is_empty() {
            format!("{}:{}", self.namespace, self.name)
        } else {
            let labels_str = self
                .labels
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            format!("{}:{}[{}]", self.namespace, self.name, labels_str)
        }
    }
}

impl Display for MetricId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_formatted())
    }
}

/// The fundamental type of a metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    /// A value that only ever increases or resets to zero (e.g., total requests).
    Counter,
    /// A value that can go up or down (e.g., current memory usage).
    Gauge,
    /// A value that tracks the distribution of a set of measurements.
    Histogram,
}

/// An enumeration of possible metric values.
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// A 64-bit unsigned integer for counters.
    Counter(u64),
    /// A 64-bit float for gauges.
    Gauge(f64),
    /// A collection of samples and their distribution across predefined buckets.
    Histogram {
        /// The raw samples recorded.
        samples: Vec<f64>,
        /// The upper bounds of the histogram buckets.
        bucket_bounds: Vec<f64>,
        /// The count of samples within each bucket.
        bucket_counts: Vec<u64>,
    },
}

impl MetricValue {
    /// Returns the [`MetricType`] corresponding to this value.
    pub fn metric_type(&self) -> MetricType {
        match self {
            MetricValue::Counter(_) => MetricType::Counter,
            MetricValue::Gauge(_) => MetricType::Gauge,
            MetricValue::Histogram { .. } => MetricType::Histogram,
        }
    }

    /// Returns the value as an `f64` if it is a `Counter` or `Gauge`.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValue::Counter(v) => Some(*v as f64),
            MetricValue::Gauge(v) => Some(*v),
            MetricValue::Histogram { .. } => None,
        }
    }

    /// Returns the value as a `u64` if it is a `Counter`.
    pub fn as_counter(&self) -> Option<u64> {
        match self {
            MetricValue::Counter(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as an `f64` if it is a `Gauge`.
    pub fn as_gauge(&self) -> Option<f64> {
        match self {
            MetricValue::Gauge(v) => Some(*v),
            _ => None,
        }
    }
}

/// Descriptive, static metadata about a metric.
#[derive(Debug, Clone)]
pub struct MetricMetadata {
    /// The metric's unique identifier.
    pub id: MetricId,
    /// The type of the metric.
    pub metric_type: MetricType,
    /// A human-readable description of what the metric measures.
    pub description: String,
    /// The unit of measurement (e.g., "ms", "bytes").
    pub unit: String,
    /// The timestamp when this metric was first registered.
    pub created_at: Instant,
    /// The timestamp when this metric was last updated.
    pub last_updated: Instant,
}

impl MetricMetadata {
    /// Creates new metadata for a metric.
    pub fn new(
        id: MetricId,
        metric_type: MetricType,
        description: impl Into<String>,
        unit: impl Into<String>,
    ) -> Self {
        let now = Instant::now();
        Self {
            id,
            metric_type,
            description: description.into(),
            unit: unit.into(),
            created_at: now,
            last_updated: now,
        }
    }

    /// Updates the `last_updated` timestamp to the current time.
    pub fn update_timestamp(&mut self) {
        self.last_updated = Instant::now();
    }
}

/// A complete metric entry, combining its value with its descriptive metadata.
#[derive(Debug, Clone)]
pub struct Metric {
    /// The static, descriptive metadata for the metric.
    pub metadata: MetricMetadata,
    /// The current, dynamic value of the metric.
    pub value: MetricValue,
}

impl Metric {
    /// A convenience constructor for creating a new `Counter` metric.
    pub fn new_counter(id: MetricId, description: impl Into<String>, initial_value: u64) -> Self {
        Self {
            metadata: MetricMetadata::new(id, MetricType::Counter, description, "count"),
            value: MetricValue::Counter(initial_value),
        }
    }

    /// A convenience constructor for creating a new `Gauge` metric.
    pub fn new_gauge(
        id: MetricId,
        description: impl Into<String>,
        unit: impl Into<String>,
        initial_value: f64,
    ) -> Self {
        Self {
            metadata: MetricMetadata::new(id, MetricType::Gauge, description, unit),
            value: MetricValue::Gauge(initial_value),
        }
    }

    /// A convenience constructor for creating a new `Histogram` metric.
    pub fn new_histogram(
        id: MetricId,
        description: impl Into<String>,
        unit: impl Into<String>,
        bucket_bounds: Vec<f64>,
    ) -> Self {
        let bucket_counts = vec![0; bucket_bounds.len()];
        Self {
            metadata: MetricMetadata::new(id, MetricType::Histogram, description, unit),
            value: MetricValue::Histogram {
                samples: Vec::new(),
                bucket_bounds,
                bucket_counts,
            },
        }
    }
}

/// A specialized `Result` type for metric-related operations.
pub type MetricsResult<T> = Result<T, MetricsError>;

/// An error that can occur within the metrics system.
#[derive(Debug, Clone)]
pub enum MetricsError {
    /// The requested metric was not found in the registry.
    MetricNotFound(MetricId),
    /// An operation was attempted on a metric of the wrong type
    /// (e.g., trying to set a gauge value on a counter).
    TypeMismatch {
        /// The expected metric type for the operation.
        expected: MetricType,
        /// The actual metric type that was found.
        found: MetricType,
    },
    /// An error originating from the backend storage layer.
    StorageError(String),
    /// An invalid operation was attempted (e.g., invalid histogram bounds).
    InvalidOperation(String),
}

impl Display for MetricsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetricsError::MetricNotFound(id) => write!(f, "Metric not found: {id}"),
            MetricsError::TypeMismatch { expected, found } => {
                write!(f, "Type mismatch: expected {expected:?}, found {found:?}")
            }
            MetricsError::StorageError(msg) => write!(f, "Storage error: {msg}"),
            MetricsError::InvalidOperation(msg) => write!(f, "Invalid operation: {msg}"),
        }
    }
}

impl std::error::Error for MetricsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_id_creation() {
        let id = MetricId::new("engine", "frame_time")
            .with_label("gpu", "nvidia")
            .with_label("quality", "high");

        assert_eq!(id.namespace, "engine");
        assert_eq!(id.name, "frame_time");
        assert_eq!(id.labels.len(), 2);

        // Labels should be sorted
        assert_eq!(id.labels[0], ("gpu".to_string(), "nvidia".to_string()));
        assert_eq!(id.labels[1], ("quality".to_string(), "high".to_string()));
    }

    #[test]
    fn test_metric_id_formatting() {
        let id1 = MetricId::new("engine", "frame_time");
        assert_eq!(id1.to_string_formatted(), "engine:frame_time");

        let id2 = MetricId::new("renderer", "triangles")
            .with_label("pass", "main")
            .with_label("quality", "high");
        assert_eq!(
            id2.to_string_formatted(),
            "renderer:triangles[pass=main,quality=high]"
        );
    }

    #[test]
    fn test_metric_value_types() {
        let counter = MetricValue::Counter(42);
        assert_eq!(counter.metric_type(), MetricType::Counter);
        assert_eq!(counter.as_counter(), Some(42));
        assert_eq!(counter.as_f64(), Some(42.0));

        use crate::math::PI;

        let gauge = MetricValue::Gauge(PI as f64);
        assert_eq!(gauge.metric_type(), MetricType::Gauge);
        assert_eq!(gauge.as_gauge(), Some(PI as f64));
        assert_eq!(gauge.as_f64(), Some(PI as f64));
    }

    #[test]
    fn test_metric_creation() {
        let id = MetricId::new("test", "counter");
        let metric = Metric::new_counter(id.clone(), "Test counter", 0);

        assert_eq!(metric.metadata.id, id);
        assert_eq!(metric.metadata.metric_type, MetricType::Counter);
        assert_eq!(metric.metadata.unit, "count");
        assert_eq!(metric.value.as_counter(), Some(0));
    }
}
