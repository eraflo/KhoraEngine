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

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::time::Instant;

/// Unique identifier for a metric in the system
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricId {
    /// Namespace for the metric (e.g., "engine", "renderer", "memory")
    pub namespace: String,
    /// Name of the metric (e.g., "frame_time_ms", "triangles_rendered")
    pub name: String,
    /// Optional labels for dimension filtering (e.g., {"gpu": "nvidia", "quality": "high"})
    pub labels: Vec<(String, String)>,
}

impl MetricId {
    /// Create a new MetricId with namespace and name
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
            labels: Vec::new(),
        }
    }

    /// Add a label to this metric ID
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        // Keep labels sorted for consistent hashing
        self.labels.sort_by(|a, b| a.0.cmp(&b.0));
        self
    }

    /// Get a formatted string representation
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

/// The type of metric being stored
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    /// A counter that only goes up (e.g., total requests, errors)
    Counter,
    /// A gauge that can go up and down (e.g., memory usage, active connections)
    Gauge,
    /// A histogram for tracking distributions (simplified for v1)
    Histogram,
}

/// A metric value that can be stored in the system
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// Counter value - monotonically increasing
    Counter(u64),
    /// Gauge value - can increase or decrease
    Gauge(f64),
    /// Histogram with buckets and samples (simplified for v1)
    Histogram {
        samples: Vec<f64>,
        bucket_bounds: Vec<f64>,
        bucket_counts: Vec<u64>,
    },
}

impl MetricValue {
    /// Get the type of this metric value
    pub fn metric_type(&self) -> MetricType {
        match self {
            MetricValue::Counter(_) => MetricType::Counter,
            MetricValue::Gauge(_) => MetricType::Gauge,
            MetricValue::Histogram { .. } => MetricType::Histogram,
        }
    }

    /// Get the numeric value as f64 (for counters and gauges)
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValue::Counter(v) => Some(*v as f64),
            MetricValue::Gauge(v) => Some(*v),
            MetricValue::Histogram { .. } => None, // Histograms don't have a single value
        }
    }

    /// Get counter value if this is a counter
    pub fn as_counter(&self) -> Option<u64> {
        match self {
            MetricValue::Counter(v) => Some(*v),
            _ => None,
        }
    }

    /// Get gauge value if this is a gauge
    pub fn as_gauge(&self) -> Option<f64> {
        match self {
            MetricValue::Gauge(v) => Some(*v),
            _ => None,
        }
    }
}

/// Metadata about a metric
#[derive(Debug, Clone)]
pub struct MetricMetadata {
    /// The metric's unique identifier
    pub id: MetricId,
    /// The type of metric
    pub metric_type: MetricType,
    /// Human-readable description
    pub description: String,
    /// Unit of measurement (e.g., "ms", "bytes", "count")
    pub unit: String,
    /// When this metric was first created
    pub created_at: Instant,
    /// When this metric was last updated
    pub last_updated: Instant,
}

impl MetricMetadata {
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

    pub fn update_timestamp(&mut self) {
        self.last_updated = Instant::now();
    }
}

/// A complete metric entry with value and metadata
#[derive(Debug, Clone)]
pub struct Metric {
    pub metadata: MetricMetadata,
    pub value: MetricValue,
}

impl Metric {
    pub fn new_counter(id: MetricId, description: impl Into<String>, initial_value: u64) -> Self {
        Self {
            metadata: MetricMetadata::new(id, MetricType::Counter, description, "count"),
            value: MetricValue::Counter(initial_value),
        }
    }

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

/// Result type for metric operations
pub type MetricsResult<T> = Result<T, MetricsError>;

/// Errors that can occur in the metrics system
#[derive(Debug, Clone)]
pub enum MetricsError {
    /// Metric with given ID not found
    MetricNotFound(MetricId),
    /// Trying to perform incompatible operation (e.g., increment a gauge as counter)
    TypeMismatch {
        expected: MetricType,
        found: MetricType,
    },
    /// Backend storage error
    StorageError(String),
    /// Invalid operation
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
