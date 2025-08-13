# Metrics System Architecture

## Overview

The KhoraEngine metrics system follows a modular and extensible architecture designed for:

- **Performance**: Thread-safe metrics collection with minimal overhead
- **Flexibility**: Support for different storage backends
- **Extensibility**: Simple API for adding new metrics
- **Configuration**: JSON support for defining metrics without recompilation

## Component Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Engine                               │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                EngineMetrics                        │    │
│  │  ┌─────────────────────────────────────────────┐    │    │
│  │  │             MetricsRegistry                 │    │    │
│  │  │  ┌─────────────────────────────────────┐    │    │    │
│  │  │  │          MetricsBackend             │    │    │    │
│  │  │  │  ┌─────────────────────────────┐    │    │    │    │
│  │  │  │  │      InMemoryBackend        │    │    │    │    │
│  │  │  │  │  RwLock<HashMap<Id,Value>>  │    │    │    │    │
│  │  │  │  └─────────────────────────────┘    │    │    │    │
│  │  │  └─────────────────────────────────────┘    │    │    │
│  │  └─────────────────────────────────────────────┘    │    │
│  │                                                     │    │
│  │  HashMap<String, MetricHandle>                      │    │
│  │  MetricsConfig                                      │    │
│  └─────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. Base Types (`types.rs`)

```rust
// Unique identifier for a metric
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MetricId {
    pub namespace: String,
    pub name: String,
    pub labels: BTreeMap<String, String>,
}

// Possible values for a metric
#[derive(Debug, Clone)]
pub enum MetricValue {
    Counter(u64),
    Gauge(f64),
    Histogram { buckets: Vec<f64>, samples: Vec<f64> },
}
```

### 2. Storage Backend (`backend.rs`, `memory_backend.rs`)

The `MetricsBackend` trait defines the interface for different storage systems:

```rust
pub trait MetricsBackend: Send + Sync + std::fmt::Debug {
    fn increment_counter(&self, id: &MetricId) -> MetricsResult<()>;
    fn set_gauge(&self, id: &MetricId, value: f64) -> MetricsResult<()>;
    fn record_histogram(&self, id: &MetricId, value: f64) -> MetricsResult<()>;
    // ...
}
```

The current `InMemoryBackend` implementation uses:
- `RwLock<HashMap<MetricId, MetricValue>>` for thread-safe storage
- Read/write separation for performance
- Robust error handling

### 3. Registry (`registry.rs`)

The `MetricsRegistry` orchestrates operations:

```rust
pub struct MetricsRegistry {
    backend: Arc<dyn MetricsBackend>,
    counter_handles: Arc<RwLock<HashMap<MetricId, CounterHandle>>>,
    gauge_handles: Arc<RwLock<HashMap<MetricId, GaugeHandle>>>,
}
```

**Responsibilities:**
- Register new metrics
- Create typed handles
- Validate operations
- Filtering and queries

### 4. JSON Configuration (`config.rs`)

Declarative configuration support:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub metrics: HashMap<String, MetricConfig>,
}

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

### 5. EngineMetrics (`engine.rs`)

Integration layer for the engine:

```rust
pub struct EngineMetrics {
    registry: MetricsRegistry,
    metric_handles: HashMap<String, MetricHandle>,
    config: Option<MetricsConfig>,
}
```

**Features:**
- Simple API for common metrics
- Automatic configuration of engine metrics
- Extensibility for user metrics
- Integration with frame stats

## Design Patterns

### 1. Handle Pattern

Metrics use typed handles for efficiency:

```rust
pub struct CounterHandle {
    id: MetricId,
    backend: Arc<dyn MetricsBackend>,
}

impl CounterHandle {
    pub fn increment(&self) -> MetricsResult<()> {
        self.backend.increment_counter(&self.id)
    }
}
```

**Advantages:**
- Validation at registration time
- Type-safe API
- Optimized performance (no lookup on each operation)

### 2. Strategy Pattern (Backend)

Different backends can be implemented:

```rust
// Current: in-memory
let backend = InMemoryBackend::new();

// Future: database
// let backend = DatabaseBackend::new(connection);

// Future: network  
// let backend = PrometheusBackend::new(endpoint);
```

### 3. Builder Pattern (Configuration)

Fluent configuration via JSON or API:

```rust
// Via configuration
let config = MetricsConfig::from_file("metrics.json")?;
metrics.initialize_with_config(config)?;

// Via API
metrics.register_custom_counter("name", "ns", "counter", "desc")?;
```

## Thread Safety

The system is fully thread-safe:

- **InMemoryBackend**: `RwLock<HashMap>` for concurrent access
- **Handles**: `Arc<dyn MetricsBackend>` shared
- **Registry**: `Arc<RwLock<>>` for handle collections

### Locking Strategy

```rust
// Read (frequent): RwLock::read()
let metrics = self.storage.read().unwrap();
let value = metrics.get(id);

// Write (less frequent): RwLock::write()  
let mut metrics = self.storage.write().unwrap();
metrics.insert(id.clone(), value);
```

## Performance

### Optimizations

1. **Cached handles**: Avoids metric lookups
2. **RwLock**: Simultaneous reads for frequent access  
3. **Arc**: Sharing without copying backends
4. **BTreeMap for labels**: Deterministic ordering and efficient comparison

### Expected Benchmarks

- **Counter increment**: ~10-50ns 
- **Gauge set**: ~10-50ns
- **Registry lookup**: ~100-200ns (first time only)

## Extensibility

### New Backends

```rust
struct PrometheusBackend {
    client: PrometheusClient,
}

impl MetricsBackend for PrometheusBackend {
    fn increment_counter(&self, id: &MetricId) -> MetricsResult<()> {
        // Send to Prometheus server
    }
}
```

### New Metric Types

```rust
// In MetricValue
pub enum MetricValue {
    Counter(u64),
    Gauge(f64), 
    Histogram { buckets: Vec<f64>, samples: Vec<f64> },
    Timer(Duration),      // New
    Rate(f64, Duration),  // New
}
```

### Aggregation and Calculations

```rust
// Computed metrics
impl EngineMetrics {
    pub fn register_computed_gauge<F>(&mut self, name: &str, compute: F) 
    where F: Fn() -> f64 + Send + Sync
    {
        // On-demand or periodic calculation
    }
}
```

## Migration and Evolution

### Compatibility

- **Stable API**: Handles remain compatible
- **Config versioning**: Support for multiple JSON versions
- **Graceful degradation**: Missing metric = no-op

### Technical Roadmap

1. **Phase 1** ✅: In-memory backend + Registry
2. **Phase 2** ✅: JSON configuration + EngineMetrics  
3. **Phase 3** 🔄: External backends (Prometheus, InfluxDB)
4. **Phase 4** 📋: Aggregation and computed metrics
5. **Phase 5** 📋: Streaming and real-time export

This architecture enables progressive evolution while maintaining a simple and performant API for common use cases.
