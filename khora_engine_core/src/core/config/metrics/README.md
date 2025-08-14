# Memory Monitoring Configuration

This directory contains ready-to-use JSON configurations for KhoraEngine's memory monitoring system.

## Available Configurations

### 1. `memory_basic.json`
Minimal configuration for essential memory monitoring.

**Included metrics:**
- Current memory usage and peak
- Allocation/deallocation counters
- Fragmentation ratio

**Recommended usage:** Simple applications, prototypes, or when you just want to monitor basic memory usage.

### 2. `memory_extended.json`
Complete configuration with all extended memory statistics.

**Included metrics:**
- All basic metrics
- Lifetime statistics (total allocations/deallocations)
- Size categorization (small/large allocations)
- Performance metrics (efficiency, turnover rate)
- Fragmentation and allocation averages

**Recommended usage:** Production applications requiring detailed memory monitoring.

### 3. `memory_advanced.json`
Advanced configuration with histograms for performance analysis.

**Included metrics:**
- All extended metrics
- Allocation size distribution histograms
- Memory utilization distribution histograms
- Labels for categorization
- Reallocation metrics

**Recommended usage:** Advanced profiling, performance optimization, detailed allocation pattern analysis.
### 4. `monitoring_complete.json`
Complete configuration including memory, GPU, VRAM and performance.

**Included metrics:**
- System memory monitoring (RAM)
- VRAM monitoring
- GPU metrics (frame time, main pass)
- CPU metrics (preparation, submission)
- Frame counters and FPS
- Labels to differentiate resource types

**Recommended usage:** Complete applications requiring global monitoring of all resources.

## Usage in Code

### Simple Method with Presets (Recommended)

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;
use khora_engine_core::core::metrics::config::{MemoryMonitoringPreset, MonitoringPreset};

// Create directly with a memory preset
let metrics = EngineMetrics::with_memory_preset(MemoryMonitoringPreset::Extended)?;

// Or create with a complete preset
let metrics = EngineMetrics::with_preset(MonitoringPreset::Complete)?;

// Or with a memory preset via MonitoringPreset
let metrics = EngineMetrics::with_preset(
    MonitoringPreset::Memory(MemoryMonitoringPreset::Advanced)
)?;

// Add presets to an existing configuration
let mut metrics = EngineMetrics::with_default_config();
metrics.extend_from_memory_preset(MemoryMonitoringPreset::Basic)?;
metrics.extend_from_preset(MonitoringPreset::Complete)?;
```

### List Available Presets

```rust
use khora_engine_core::core::metrics::config::MetricsConfig;

// List all memory presets
for (preset, description, metric_count) in MetricsConfig::list_memory_presets() {
    println!("{:?}: {} ({} metrics)", preset, description, metric_count);
}

// List all presets
for (preset, description, metric_count) in MetricsConfig::list_presets() {
    println!("{:?}: {} ({} metrics)", preset, description, metric_count);
}
```

### Loading a Configuration (Advanced Method)

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

// Load a specific configuration
let mut metrics = EngineMetrics::new();
metrics.extend_from_file("khora_engine_core/src/core/config/metrics/memory_extended.json")?;

// Or use the default configuration and add metrics
let mut metrics = EngineMetrics::with_default_config();
metrics.extend_from_file("khora_engine_core/src/core/config/metrics/memory_extended.json")?;

// Metrics are automatically configured and ready to use
// The engine will automatically update them
```

### Extension with Custom Metrics

```rust
// After loading a base configuration, add custom metrics
let mut metrics = EngineMetrics::with_default_config();
metrics.extend_from_file("khora_engine_core/src/core/config/metrics/memory_basic.json")?;

metrics.register_custom_gauge(
    "my_custom_memory", 
    "game", 
    "memory.custom_tracker", 
    "Custom memory tracking"
)?;

// Use the custom metric
metrics.update_gauge("my_custom_memory", 42.0)?;
```

### Dynamic Modification

```rust
// Configurations can be modified and saved
let mut config = MetricsConfig::from_file("khora_engine_core/src/core/config/metrics/memory_basic.json")?;

// Add a new metric to the configuration
config.metrics.insert("new_metric".to_string(), MetricConfig {
    namespace: "engine".to_string(),
    name: "memory.new_metric".to_string(),
    description: "New memory metric".to_string(),
    metric_type: "gauge".to_string(),
    unit: Some("bytes".to_string()),
    buckets: None,
    labels: None,
});

// Save the modified configuration
config.to_file("khora_engine_core/src/core/config/metrics/memory_custom.json")?;
```

## Metrics Structure

### Naming Conventions

- **Namespace**: `engine` for all system metrics
- **Name**: Hierarchical format with dots (e.g., `memory.usage_mb`, `performance.gpu_frame_time_ms`)
- **Units**: Clearly specified (`megabytes`, `milliseconds`, `percentage`, etc.)

### Metric Types

- **Gauge**: Values that can go up/down (memory usage, fragmentation)
- **Counter**: Values that only increase (total allocations, frames rendered)
- **Histogram**: Value distributions (allocation sizes, frame times)

### Labels

Used to categorize and filter metrics:
- `resource_type`: "system_ram", "vram", "gpu", "cpu"
- `category`: "small", "medium", "large" (for allocations)
- `threshold`: Specific thresholds for categories

## Performance

- `basic` and `extended` configurations have minimal performance impact
- `advanced` configuration with histograms has slightly higher overhead
- All metrics are updated automatically by the engine during `engine.update()`

## Integration with Resource Monitors

These JSON configurations work automatically with the integrated monitoring system:

- `MemoryResourceMonitor` → Automatically updates all memory metrics
- `GpuMonitor` → Automatically updates GPU metrics
- `VramResourceMonitor` → Automatically updates VRAM metrics

No manual integration configuration is needed - the engine handles everything automatically.
