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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a single metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricConfig {
    /// Namespace for the metric (e.g., "engine", "renderer")
    pub namespace: String,
    /// Name of the metric (e.g., "frames.total", "memory.usage_mb")
    pub name: String,
    /// Description of what this metric measures
    pub description: String,
    /// Type of metric: "counter", "gauge", or "histogram"
    pub metric_type: String,
    /// Unit for gauge metrics (e.g., "megabytes", "milliseconds", "fps")
    pub unit: Option<String>,
    /// Buckets for histogram metrics
    pub buckets: Option<Vec<f64>>,
    /// Labels to attach to this metric
    pub labels: Option<HashMap<String, String>>,
}

/// Complete metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Map of metric ID to configuration
    pub metrics: HashMap<String, MetricConfig>,
}

impl MetricsConfig {
    /// Load metrics configuration from JSON string
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Load metrics configuration from JSON file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(Self::from_json(&content)?)
    }

    /// Save metrics configuration to JSON file
    pub fn to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Get default engine metrics configuration
    pub fn default_engine_metrics() -> Self {
        let mut metrics = HashMap::new();

        // Frame counter
        metrics.insert(
            "frame_counter".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "frames.total".to_string(),
                description: "Total frames rendered".to_string(),
                metric_type: "counter".to_string(),
                unit: None,
                buckets: None,
                labels: None,
            },
        );

        // Memory gauge
        metrics.insert(
            "memory_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "memory.usage_mb".to_string(),
                description: "Memory usage in MB".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("megabytes".to_string()),
                buckets: None,
                labels: None,
            },
        );

        // Performance metrics
        metrics.insert(
            "frame_time_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "performance.frame_time_ms".to_string(),
                description: "Frame time in milliseconds".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("milliseconds".to_string()),
                buckets: None,
                labels: None,
            },
        );

        metrics.insert(
            "cpu_time_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "performance.cpu_time_ms".to_string(),
                description: "CPU time per frame in milliseconds".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("milliseconds".to_string()),
                buckets: None,
                labels: None,
            },
        );

        metrics.insert(
            "gpu_time_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "performance.gpu_time_ms".to_string(),
                description: "GPU time per frame in milliseconds".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("milliseconds".to_string()),
                buckets: None,
                labels: None,
            },
        );

        metrics.insert(
            "fps_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "performance.fps".to_string(),
                description: "Frames per second".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("fps".to_string()),
                buckets: None,
                labels: None,
            },
        );

        metrics.insert(
            "gpu_main_pass_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "performance.gpu_main_pass_ms".to_string(),
                description: "GPU main pass time in milliseconds".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("milliseconds".to_string()),
                buckets: None,
                labels: None,
            },
        );

        // Rendering metrics
        metrics.insert(
            "draw_calls_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "rendering.draw_calls".to_string(),
                description: "Number of draw calls per frame".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("count".to_string()),
                buckets: None,
                labels: None,
            },
        );

        metrics.insert(
            "triangles_gauge".to_string(),
            MetricConfig {
                namespace: "engine".to_string(),
                name: "rendering.triangles".to_string(),
                description: "Number of triangles rendered per frame".to_string(),
                metric_type: "gauge".to_string(),
                unit: Some("count".to_string()),
                buckets: None,
                labels: None,
            },
        );

        Self { metrics }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_engine_metrics_config() {
        let config = MetricsConfig::default_engine_metrics();

        assert!(!config.metrics.is_empty());
        assert!(config.metrics.contains_key("frame_counter"));
        assert!(config.metrics.contains_key("memory_gauge"));
        assert!(config.metrics.contains_key("fps_gauge"));
    }

    #[test]
    fn test_json_serialization() {
        let config = MetricsConfig::default_engine_metrics();

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: MetricsConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.metrics.len(), deserialized.metrics.len());
    }

    #[test]
    fn test_from_json() {
        let json = r#"
        {
            "metrics": {
                "test_counter": {
                    "namespace": "test",
                    "name": "counter.total",
                    "description": "Test counter",
                    "metric_type": "counter"
                }
            }
        }"#;

        let config = MetricsConfig::from_json(json).unwrap();
        assert_eq!(config.metrics.len(), 1);
        assert!(config.metrics.contains_key("test_counter"));
    }
}
