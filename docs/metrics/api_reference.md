# Metrics System API Reference

This document provides a complete API reference for the KhoraEngine metrics system.

## Core Types

### MetricId

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricId {
    pub namespace: String,
    pub name: String,
    pub labels: BTreeMap<String, String>,
}
```

Uniquely identifies a metric within the system.

**Fields:**
- `namespace`: Logical grouping (e.g., "engine", "game", "network")
- `name`: Metric name (e.g., "frames.total", "player.score")
- `labels`: Optional key-value pairs for additional categorization

### MetricValue

```rust
#[derive(Debug, Clone)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    Histogram {
        buckets: Vec<f64>,
        samples: Vec<f64>,
    },
}
```

Represents the different types of metric values supported.

### MetricsResult

```rust
pub type MetricsResult<T> = Result<T, MetricsError>;
```

Standard result type for metrics operations.

### MetricsError

```rust
#[derive(Debug)]
pub enum MetricsError {
    MetricNotFound(String),
    TypeMismatch { expected: String, found: String },
    InvalidOperation(String),
    BackendError(String),
}
```

## EngineMetrics

The main interface for metrics management within the engine.

### Creation

#### `new() -> Self`

Creates a new, uninitialized EngineMetrics instance.

```rust
let metrics = EngineMetrics::new();
```

#### `with_default_config() -> Self`

Creates a new EngineMetrics instance with default engine metrics pre-configured.

```rust
let metrics = EngineMetrics::with_default_config();
```

**Default metrics include:**
- `frame_counter`: Total frames rendered
- `fps_gauge`: Current FPS
- `memory_gauge`: Memory usage in MB
- `frame_time_gauge`: Frame time in milliseconds
- `cpu_time_gauge`: CPU time per frame
- `gpu_time_gauge`: GPU time per frame
- `gpu_main_pass_gauge`: GPU main pass time
- `draw_calls_gauge`: Draw calls per frame
- `triangles_gauge`: Triangles rendered per frame

### Configuration

#### `initialize_with_config(config: MetricsConfig) -> MetricsResult<()>`

Initializes metrics from a configuration object.

```rust
let config = MetricsConfig::default_engine_metrics();
metrics.initialize_with_config(config)?;
```

#### `initialize_from_file(config_path: &str) -> Result<(), Box<dyn std::error::Error>>`

Initializes metrics from a JSON configuration file.

```rust
metrics.initialize_from_file("metrics_config.json")?;
```

#### `extend_from_file(config_path: &str) -> Result<(), Box<dyn std::error::Error>>`

Adds additional metrics from a JSON configuration file without replacing existing ones.

```rust
let mut metrics = EngineMetrics::with_default_config();
metrics.extend_from_file("custom_metrics.json")?;
```

### Custom Metric Registration

#### `register_custom_counter(handle_key: &str, namespace: &str, name: &str, description: &str) -> MetricsResult<()>`

Registers a new counter metric.

```rust
metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Number of player deaths")?;
```

**Parameters:**
- `handle_key`: Unique identifier for this metric handle
- `namespace`: Metric namespace
- `name`: Metric name
- `description`: Human-readable description

#### `register_custom_gauge(handle_key: &str, namespace: &str, name: &str, description: &str, unit: &str) -> MetricsResult<()>`

Registers a new gauge metric.

```rust
metrics.register_custom_gauge("player_health", "game", "player.health", "Player's current health", "percentage")?;
```

**Parameters:**
- `handle_key`: Unique identifier for this metric handle
- `namespace`: Metric namespace
- `name`: Metric name
- `description`: Human-readable description
- `unit`: Unit of measurement

#### `register_custom_counter_with_labels(handle_key: &str, namespace: &str, name: &str, description: &str, labels: Vec<(String, String)>) -> MetricsResult<()>`

Registers a new counter metric with labels.

```rust
let labels = vec![("level".to_string(), "1".to_string())];
metrics.register_custom_counter_with_labels("level_attempts", "game", "level.attempts", "Level attempts", labels)?;
```

#### `register_custom_gauge_with_labels(handle_key: &str, namespace: &str, name: &str, description: &str, unit: &str, labels: Vec<(String, String)>) -> MetricsResult<()>`

Registers a new gauge metric with labels.

```rust
let labels = vec![("region".to_string(), "us-east".to_string())];
metrics.register_custom_gauge_with_labels("regional_latency", "network", "latency.regional", "Regional latency", "ms", labels)?;
```

### Metric Updates

#### `update_metric(handle_key: &str, value: f64)`

Updates a metric by its handle key. For counters, increments by 1 (value ignored). For gauges, sets the value.

```rust
metrics.update_metric("player_health", 85.0);
```

#### `increment_counter(handle_key: &str, amount: u64)`

Increments a counter by a specific amount.

```rust
metrics.increment_counter("player_deaths", 1);
metrics.increment_counter("bullets_fired", 5);
```

#### `set_gauge(handle_key: &str, value: f64)`

Sets a gauge to a specific value.

```rust
metrics.set_gauge("player_health", 75.5);
metrics.set_gauge("temperature", 68.2);
```

### Engine Integration

#### `update_basic(frame_time_ms: f64, cpu_time_ms: f64, gpu_time_ms: f64)`

Updates basic engine metrics with performance data.

```rust
metrics.update_basic(16.67, 12.5, 4.2);
```

#### `update_all(stats: &FrameStats)`

Updates all engine metrics with comprehensive frame statistics.

```rust
let stats = FrameStats {
    fps: 60,
    memory_usage_kib: 1024,
    render_duration_us: 16670,
    gpu_main_pass_ms: 12.5,
    gpu_frame_total_ms: 15.0,
    draw_calls: 150,
    triangles: 50000,
};
metrics.update_all(&stats);
```

### Inspection and Debugging

#### `is_initialized() -> bool`

Returns whether the metrics system has been initialized.

```rust
if metrics.is_initialized() {
    println!("Metrics are ready");
}
```

#### `get_metrics_snapshot() -> Vec<String>`

Returns a snapshot of all current metric values as formatted strings.

```rust
let snapshot = metrics.get_metrics_snapshot();
for metric in snapshot {
    println!("{}", metric);
}
```

#### `log_metrics_summary()`

Logs all current metrics to the console in a formatted summary.

```rust
metrics.log_metrics_summary();
// Output:
// === Engine Metrics Summary ===
//   engine.frames.total: Total frames rendered = Counter(1547)
//   engine.performance.fps: Frames per second = Gauge(60.0)
//   ...
// === End Metrics Summary ===
```

#### `registry() -> &MetricsRegistry`

Returns a reference to the underlying metrics registry for advanced operations.

```rust
let registry = metrics.registry();
let all_counters = registry.get_all_counters();
```

## MetricsRegistry

Low-level registry for advanced metric management.

### Creation

#### `new() -> Self`

Creates a new metrics registry with an in-memory backend.

```rust
let registry = MetricsRegistry::new();
```

### Metric Registration

#### `register_counter(namespace: &str, name: &str, description: &str) -> MetricsResult<CounterHandle>`

Registers a new counter and returns a handle.

```rust
let counter = registry.register_counter("game", "score", "Player score")?;
```

#### `register_gauge(namespace: &str, name: &str, description: &str, unit: &str) -> MetricsResult<GaugeHandle>`

Registers a new gauge and returns a handle.

```rust
let gauge = registry.register_gauge("engine", "fps", "Frames per second", "fps")?;
```

#### `register_counter_with_labels(namespace: &str, name: &str, description: &str, labels: Vec<(String, String)>) -> MetricsResult<CounterHandle>`

Registers a counter with labels.

#### `register_gauge_with_labels(namespace: &str, name: &str, description: &str, unit: &str, labels: Vec<(String, String)>) -> MetricsResult<GaugeHandle>`

Registers a gauge with labels.

### Data Retrieval

#### `get_all_counters() -> Vec<CounterMetric>`

Returns all registered counter metrics with their current values.

#### `get_all_gauges() -> Vec<GaugeMetric>`

Returns all registered gauge metrics with their current values.

#### `get_counters_by_namespace(namespace: &str) -> Vec<CounterMetric>`

Returns counters filtered by namespace.

#### `get_gauges_by_namespace(namespace: &str) -> Vec<GaugeMetric>`

Returns gauges filtered by namespace.

### Management

#### `clear_all() -> MetricsResult<()>`

Clears all metrics from the registry.

```rust
registry.clear_all()?;
```

## Metric Handles

### CounterHandle

Handle for efficiently updating counter metrics.

#### `increment() -> MetricsResult<()>`

Increments the counter by 1.

```rust
counter.increment()?;
```

### GaugeHandle

Handle for efficiently updating gauge metrics.

#### `set(value: f64) -> MetricsResult<()>`

Sets the gauge to a specific value.

```rust
gauge.set(42.5)?;
```

## Configuration Types

### MetricsConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub metrics: HashMap<String, MetricConfig>,
}
```

Top-level configuration structure for defining metrics.

#### `default_engine_metrics() -> Self`

Returns the default configuration for engine metrics.

#### `from_json(json: &str) -> Result<Self, serde_json::Error>`

Parses configuration from JSON string.

#### `from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>>`

Loads configuration from JSON file.

### MetricConfig

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricConfig {
    pub namespace: String,
    pub name: String,
    pub description: String,
    pub metric_type: String,
    pub unit: Option<String>,
    pub labels: Option<HashMap<String, String>>,
}
```

Configuration for a single metric.

**Fields:**
- `namespace`: Metric namespace
- `name`: Metric name
- `description`: Human-readable description
- `metric_type`: "counter" or "gauge"
- `unit`: Optional unit of measurement
- `labels`: Optional key-value labels

## FrameStats

```rust
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
```

Comprehensive frame statistics for engine metrics updates.

## Usage Patterns

### Basic Usage

```rust
// Create with defaults
let mut metrics = EngineMetrics::with_default_config();

// Add custom metrics
metrics.register_custom_counter("events", "game", "events.triggered", "Game events")?;
metrics.register_custom_gauge("temperature", "system", "cpu.temp", "CPU temperature", "celsius")?;

// Update metrics
metrics.increment_counter("events", 1);
metrics.set_gauge("temperature", 72.5);
```

### Configuration-Based

```rust
// Load from file
let mut metrics = EngineMetrics::new();
metrics.initialize_from_file("metrics.json")?;

// Or extend defaults
let mut metrics = EngineMetrics::with_default_config();
metrics.extend_from_file("custom_metrics.json")?;
```

### Error Handling

```rust
match metrics.register_custom_counter("test", "game", "test.counter", "Test") {
    Ok(()) => println!("Counter registered successfully"),
    Err(MetricsError::InvalidOperation(msg)) => eprintln!("Invalid operation: {}", msg),
    Err(e) => eprintln!("Error: {:?}", e),
}
```

This API provides a complete interface for metrics management with both high-level convenience methods and low-level control for advanced use cases.
