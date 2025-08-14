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

use crate::core::metrics::config::{MetricConfig, MetricsConfig};
use crate::core::metrics::{CounterHandle, GaugeHandle, MetricsRegistry, MetricsResult};
use crate::memory::get_currently_allocated_bytes;
use std::collections::HashMap;

/// Frame statistics for metrics updates
#[derive(Debug, Clone)]
pub struct FrameStats {
    pub fps: u32,
    pub memory_usage_kib: u64,
    pub render_duration_us: u64,
    pub gpu_main_pass_ms: f32,
    pub gpu_frame_total_ms: f32,
    pub draw_calls: u32,
    pub triangles: u32,
}

/// Wrapper for different types of metric handles
#[derive(Debug)]
pub enum MetricHandle {
    Counter(CounterHandle),
    Gauge(GaugeHandle),
    // Future: Histogram(HistogramHandle),
}

/// Engine-specific metrics manager
///
/// This struct manages all metrics related to engine performance and rendering.
/// It contains its own MetricsRegistry and provides a clean API for configuring
/// and updating metrics based on JSON configuration.
///
/// **Note**: Configuration management functionality is currently integrated here
/// for convenience but is considered temporary. Future versions will move this
/// to a dedicated configuration manager system.
///
/// # Custom Metrics
///
/// Users can extend the metrics system by:
/// 1. Creating custom configuration files with additional metrics
/// 2. Using `register_custom_metric()` to add metrics programmatically
/// 3. Using `update_metric()` to update any metric by its handle key
///
/// # Example
///
/// ```rust
/// use khora_engine_core::core::metrics::engine::EngineMetrics;
///
/// let mut metrics = EngineMetrics::with_default_config();
///
/// // Add a custom metric
/// metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Number of player deaths");
///
/// // Update it
/// metrics.increment_counter("player_deaths", 1);
/// ```
#[derive(Debug)]
pub struct EngineMetrics {
    // The metrics registry - owned by this struct
    registry: MetricsRegistry,
    // Dynamic metric handles based on configuration
    metric_handles: HashMap<String, MetricHandle>,
    // TEMPORARY: Configuration field
    // TODO: Remove when configuration management is moved to dedicated config manager
    config: Option<MetricsConfig>,
}

impl EngineMetrics {
    /// Creates a new EngineMetrics instance
    ///
    /// **Note**: For convenience, consider using `with_default_config()` instead,
    /// though the configuration functionality is temporary and will be moved
    /// to a dedicated configuration manager in the future.
    pub fn new() -> Self {
        Self {
            registry: MetricsRegistry::new(),
            metric_handles: HashMap::new(),
            config: None,
        }
    }

    /// Creates a new EngineMetrics instance with default configuration
    ///
    /// **Note**: This configuration functionality is temporary and will be moved
    /// to a dedicated configuration manager in the future.
    pub fn with_default_config() -> Self {
        let mut metrics = Self::new();
        let config = MetricsConfig::default_engine_metrics();
        metrics.initialize_with_config(config).unwrap_or_else(|e| {
            log::error!("Failed to initialize default metrics config: {e}");
        });
        metrics
    }

    /// Initialize metrics from configuration
    ///
    /// **Note**: This configuration functionality is temporary and will be moved
    /// to a dedicated configuration manager in the future.
    pub fn initialize_with_config(&mut self, config: MetricsConfig) -> MetricsResult<()> {
        self.config = Some(config.clone());

        for (handle_key, metric_config) in &config.metrics {
            let handle = self.register_metric_from_config(metric_config)?;
            self.metric_handles.insert(handle_key.clone(), handle);
        }

        Ok(())
    }

    /// Initialize metrics from JSON file
    pub fn initialize_from_file(
        &mut self,
        config_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config = MetricsConfig::from_file(config_path)?;
        self.initialize_with_config(config)?;
        Ok(())
    }

    /// Register a custom counter programmatically
    ///
    /// This allows users to add their own counters without modifying configuration files.
    ///
    /// # Arguments
    /// * `handle_key` - Unique identifier for this metric (used for updates)
    /// * `namespace` - Metric namespace (e.g., "game", "network", "audio")
    /// * `name` - Metric name (e.g., "frames.rendered", "packets.sent")
    /// * `description` - Human-readable description
    ///
    /// # Example
    /// ```rust
    /// use khora_engine_core::core::metrics::engine::EngineMetrics;
    /// let mut metrics = EngineMetrics::with_default_config();
    /// metrics.register_custom_counter("player_score", "game", "player.score", "Player's current score");
    /// ```
    pub fn register_custom_counter(
        &mut self,
        handle_key: &str,
        namespace: &str,
        name: &str,
        description: &str,
    ) -> MetricsResult<()> {
        let handle = self
            .registry
            .register_counter(namespace, name, description)?;
        self.metric_handles
            .insert(handle_key.to_string(), MetricHandle::Counter(handle));
        Ok(())
    }

    /// Register a custom gauge programmatically
    ///
    /// # Arguments
    /// * `handle_key` - Unique identifier for this metric (used for updates)
    /// * `namespace` - Metric namespace (e.g., "game", "network", "audio")
    /// * `name` - Metric name (e.g., "temperature", "bandwidth", "volume")
    /// * `description` - Human-readable description
    /// * `unit` - Unit of measurement (e.g., "celsius", "mbps", "percent")
    ///
    /// # Example
    /// ```rust
    /// use khora_engine_core::core::metrics::engine::EngineMetrics;
    /// let mut metrics = EngineMetrics::with_default_config();
    /// metrics.register_custom_gauge("cpu_temp", "system", "cpu.temperature", "CPU temperature", "celsius");
    /// ```
    pub fn register_custom_gauge(
        &mut self,
        handle_key: &str,
        namespace: &str,
        name: &str,
        description: &str,
        unit: &str,
    ) -> MetricsResult<()> {
        let handle = self
            .registry
            .register_gauge(namespace, name, description, unit)?;
        self.metric_handles
            .insert(handle_key.to_string(), MetricHandle::Gauge(handle));
        Ok(())
    }

    /// Register a custom counter with labels
    ///
    /// # Arguments
    /// * `handle_key` - Unique identifier for this metric
    /// * `namespace` - Metric namespace
    /// * `name` - Metric name
    /// * `description` - Human-readable description
    /// * `labels` - Vector of (key, value) label pairs
    ///
    /// # Example
    /// ```rust
    /// use khora_engine_core::core::metrics::engine::EngineMetrics;
    /// let mut metrics = EngineMetrics::with_default_config();
    /// let labels = vec![("level".to_string(), "1".to_string()), ("difficulty".to_string(), "hard".to_string())];
    /// metrics.register_custom_counter_with_labels("enemy_kills", "game", "enemies.killed", "Enemies killed", labels);
    /// ```
    pub fn register_custom_counter_with_labels(
        &mut self,
        handle_key: &str,
        namespace: &str,
        name: &str,
        description: &str,
        labels: Vec<(String, String)>,
    ) -> MetricsResult<()> {
        let handle =
            self.registry
                .register_counter_with_labels(namespace, name, description, labels)?;
        self.metric_handles
            .insert(handle_key.to_string(), MetricHandle::Counter(handle));
        Ok(())
    }

    /// Register a custom gauge with labels
    pub fn register_custom_gauge_with_labels(
        &mut self,
        handle_key: &str,
        namespace: &str,
        name: &str,
        description: &str,
        unit: &str,
        labels: Vec<(String, String)>,
    ) -> MetricsResult<()> {
        let handle =
            self.registry
                .register_gauge_with_labels(namespace, name, description, unit, labels)?;
        self.metric_handles
            .insert(handle_key.to_string(), MetricHandle::Gauge(handle));
        Ok(())
    }

    // ============================================================================
    // TEMPORARY: Configuration management methods
    // TODO: Move all configuration-related functionality to a dedicated
    // configuration manager when the configuration system is redesigned.
    //
    // Functions to move:
    // - extend_from_file()
    // - initialize_with_config()
    // - register_metric_from_config()
    //
    // Fields to move:
    // - config: Option<MetricsConfig>
    // - metric_handles (configuration-based ones)
    // ============================================================================

    /// Load and merge additional metrics from a configuration file
    ///
    /// **Note**: This configuration functionality is temporary and will be moved
    /// to a dedicated configuration manager in the future.
    ///
    /// This allows users to extend the default engine metrics with their own
    /// custom metrics defined in JSON configuration files.
    ///
    /// # Arguments
    /// * `config_path` - Path to the JSON configuration file
    ///
    /// # Example
    /// ```rust
    /// use khora_engine_core::core::metrics::engine::EngineMetrics;
    /// // After creating with default config
    /// let mut metrics = EngineMetrics::with_default_config();
    /// // Add custom metrics from file
    /// // metrics.extend_from_file("khora_engine_core/src/core/config/metrics/memory_basic.json")?;
    /// ```
    pub fn extend_from_file(
        &mut self,
        config_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let additional_config = MetricsConfig::from_file(config_path)?;

        // Register additional metrics
        for (handle_key, metric_config) in &additional_config.metrics {
            if !self.metric_handles.contains_key(handle_key) {
                let handle = self.register_metric_from_config(metric_config)?;
                self.metric_handles.insert(handle_key.clone(), handle);
            } else {
                log::warn!("Metric handle '{handle_key}' already exists, skipping");
            }
        }

        Ok(())
    }

    /// Register a single metric from configuration
    ///
    /// **Note**: This configuration functionality is temporary and will be moved
    /// to a dedicated configuration manager in the future.
    fn register_metric_from_config(&self, config: &MetricConfig) -> MetricsResult<MetricHandle> {
        match config.metric_type.as_str() {
            "counter" => {
                let handle = if let Some(labels) = &config.labels {
                    let labels_vec: Vec<(String, String)> =
                        labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    self.registry.register_counter_with_labels(
                        &config.namespace,
                        &config.name,
                        &config.description,
                        labels_vec,
                    )?
                } else {
                    self.registry.register_counter(
                        &config.namespace,
                        &config.name,
                        &config.description,
                    )?
                };
                Ok(MetricHandle::Counter(handle))
            }
            "gauge" => {
                let unit = config.unit.as_deref().unwrap_or("count");
                let handle = if let Some(labels) = &config.labels {
                    let labels_vec: Vec<(String, String)> =
                        labels.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    self.registry.register_gauge_with_labels(
                        &config.namespace,
                        &config.name,
                        &config.description,
                        unit,
                        labels_vec,
                    )?
                } else {
                    self.registry.register_gauge(
                        &config.namespace,
                        &config.name,
                        &config.description,
                        unit,
                    )?
                };
                Ok(MetricHandle::Gauge(handle))
            }
            _ => Err(crate::core::metrics::MetricsError::InvalidOperation(
                format!("Unsupported metric type: {}", config.metric_type),
            )),
        }
    }

    /// Get a reference to the metrics registry
    pub fn registry(&self) -> &MetricsRegistry {
        &self.registry
    }

    /// Returns whether the metrics system is initialized
    pub fn is_initialized(&self) -> bool {
        !self.metric_handles.is_empty()
    }

    /// Update a specific metric by its handle key
    pub fn update_metric(&self, handle_key: &str, value: f64) {
        if let Some(handle) = self.metric_handles.get(handle_key) {
            match handle {
                MetricHandle::Counter(_) => {
                    // For counters, we increment by 1 (value is ignored for counters)
                    self.increment_counter(handle_key, 1);
                }
                MetricHandle::Gauge(gauge) => {
                    let _ = gauge.set(value);
                }
            }
        }
    }

    /// Increment a counter by a specific amount
    pub fn increment_counter(&self, handle_key: &str, amount: u64) {
        if let Some(MetricHandle::Counter(counter)) = self.metric_handles.get(handle_key) {
            for _ in 0..amount {
                let _ = counter.increment();
            }
        }
    }

    /// Set a gauge value
    pub fn set_gauge(&self, handle_key: &str, value: f64) {
        if let Some(MetricHandle::Gauge(gauge)) = self.metric_handles.get(handle_key) {
            let _ = gauge.set(value);
        }
    }

    /// Updates engine metrics with basic performance data
    pub fn update_basic(&self, frame_time_ms: f64, cpu_time_ms: f64, gpu_time_ms: f64) {
        // Increment frame counter
        self.increment_counter("frame_counter", 1);

        // Update performance metrics
        self.set_gauge("frame_time_gauge", frame_time_ms);
        self.set_gauge("cpu_time_gauge", cpu_time_ms);
        self.set_gauge("gpu_time_gauge", gpu_time_ms);

        // Update memory usage
        let memory_usage_mb = (get_currently_allocated_bytes() as f64) / (1024.0 * 1024.0);
        self.set_gauge("memory_gauge", memory_usage_mb);
    }

    /// Updates all engine metrics with comprehensive frame statistics
    pub fn update_all(&self, stats: &FrameStats) {
        // Performance metrics
        let frame_time_ms = stats.render_duration_us as f64 / 1000.0;
        let cpu_time_ms = stats.render_duration_us as f64 / 1000.0;
        let gpu_time_ms = stats.gpu_frame_total_ms as f64;

        self.set_gauge("frame_time_gauge", frame_time_ms);
        self.set_gauge("cpu_time_gauge", cpu_time_ms);
        self.set_gauge("gpu_time_gauge", gpu_time_ms);
        self.set_gauge("fps_gauge", stats.fps as f64);
        self.set_gauge("gpu_main_pass_gauge", stats.gpu_main_pass_ms as f64);

        // Memory metrics
        let memory_usage_mb = stats.memory_usage_kib as f64 / 1024.0;
        self.set_gauge("memory_gauge", memory_usage_mb);

        // Rendering metrics
        self.set_gauge("draw_calls_gauge", stats.draw_calls as f64);
        self.set_gauge("triangles_gauge", stats.triangles as f64);
    }

    /// Updates VRAM metrics specifically
    pub fn update_vram_metrics(&self, vram_usage_mb: f64, vram_peak_mb: f64) {
        self.set_gauge("vram_usage_gauge", vram_usage_mb);
        self.set_gauge("vram_peak_gauge", vram_peak_mb);
    }

    /// Gets a snapshot of all engine metrics for monitoring and debugging
    pub fn get_metrics_snapshot(&self) -> Vec<String> {
        let mut snapshot = Vec::new();

        // Get all counters and gauges
        let counters = self.registry.get_all_counters();
        let gauges = self.registry.get_all_gauges();

        // Add counters to snapshot
        for metric in counters {
            snapshot.push(format!(
                "{}.{}: {} = {:?}",
                metric.metadata.id.namespace,
                metric.metadata.id.name,
                metric.metadata.description,
                metric.value
            ));
        }

        // Add gauges to snapshot
        for metric in gauges {
            snapshot.push(format!(
                "{}.{}: {} = {:?}",
                metric.metadata.id.namespace,
                metric.metadata.id.name,
                metric.metadata.description,
                metric.value
            ));
        }

        snapshot
    }

    /// Logs a comprehensive metrics summary from the metrics system
    pub fn log_metrics_summary(&self) {
        let snapshot = self.get_metrics_snapshot();

        log::info!("=== Engine Metrics Summary ===");
        if snapshot.is_empty() {
            log::info!(
                "  No metrics found! Registry count: {}",
                self.registry.metric_count()
            );
        } else {
            log::info!("  Found {} metrics:", snapshot.len());
            for metric_line in snapshot {
                log::info!("  {metric_line}");
            }
        }
        log::info!("=== End Metrics Summary ===");
    }
}

impl Default for EngineMetrics {
    fn default() -> Self {
        Self::with_default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_metrics_creation() {
        let metrics = EngineMetrics::new();
        assert!(
            !metrics.is_initialized(),
            "Metrics should not be initialized on creation"
        );
    }

    #[test]
    fn test_engine_metrics_with_default_config() {
        let metrics = EngineMetrics::with_default_config();
        assert!(
            metrics.is_initialized(),
            "Metrics should be initialized with default config"
        );
    }

    #[test]
    fn test_engine_metrics_update_basic() {
        let metrics = EngineMetrics::with_default_config();

        // Should not panic
        metrics.update_basic(16.67, 12.5, 4.2);
    }

    #[test]
    fn test_engine_metrics_update_all() {
        let metrics = EngineMetrics::with_default_config();

        let stats = FrameStats {
            fps: 60,
            memory_usage_kib: 1024,
            render_duration_us: 16670,
            gpu_main_pass_ms: 12.5,
            gpu_frame_total_ms: 15.0,
            draw_calls: 150,
            triangles: 50000,
        };

        // Should not panic
        metrics.update_all(&stats);
    }

    #[test]
    fn test_config_based_metrics() {
        let mut metrics = EngineMetrics::new();
        let config = MetricsConfig::default_engine_metrics();

        metrics.initialize_with_config(config).unwrap();
        assert!(metrics.is_initialized());

        // Test updating individual metrics
        metrics.set_gauge("fps_gauge", 60.0);
        metrics.increment_counter("frame_counter", 1);
    }

    #[test]
    fn test_custom_metric_registration() {
        let mut metrics = EngineMetrics::with_default_config();

        // Register custom metrics
        metrics
            .register_custom_counter("test_counter", "test", "counter.test", "Test counter")
            .unwrap();
        metrics
            .register_custom_gauge("test_gauge", "test", "gauge.test", "Test gauge", "units")
            .unwrap();

        // Update them
        metrics.increment_counter("test_counter", 5);
        metrics.set_gauge("test_gauge", 42.0);

        // Should not panic
        metrics.log_metrics_summary();
    }

    #[test]
    fn test_custom_metrics_with_labels() {
        let mut metrics = EngineMetrics::with_default_config();

        let labels = vec![
            ("level".to_string(), "1".to_string()),
            ("difficulty".to_string(), "hard".to_string()),
        ];
        metrics
            .register_custom_counter_with_labels(
                "labeled_counter",
                "game",
                "test.labeled",
                "Test labeled counter",
                labels,
            )
            .unwrap();

        let gauge_labels = vec![("region".to_string(), "us-east".to_string())];
        metrics
            .register_custom_gauge_with_labels(
                "labeled_gauge",
                "network",
                "latency.regional",
                "Regional latency",
                "ms",
                gauge_labels,
            )
            .unwrap();

        // Update them
        metrics.increment_counter("labeled_counter", 1);
        metrics.set_gauge("labeled_gauge", 45.7);
    }
}
