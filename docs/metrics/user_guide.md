# Metrics System - User Guide

## Architecture

The KhoraEngine metrics system is organized in a modular and extensible way:

```
khora_engine_core/src/core/
├── config/                         # Centralized configuration
│   ├── mod.rs
│   └── example_custom_metrics.json # Example custom configuration
└── metrics/                        # Metrics system
    ├── mod.rs
    ├── types.rs                    # Base types (MetricId, MetricValue, etc.)
    ├── backend.rs                  # MetricsBackend trait
    ├── memory_backend.rs           # In-memory implementation
    ├── registry.rs                 # MetricsRegistry
    ├── config.rs                   # JSON configuration
    └── engine.rs                   # EngineMetrics (engine integration)
```

## Basic Usage

### 1. Using Default Configuration

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

// Create instance with default engine metrics
let mut metrics = EngineMetrics::with_default_config();

// The following metrics are automatically available:
// - frame_counter (frame count)
// - fps_gauge (current FPS)
// - memory_gauge (memory usage)
// - frame_time_gauge, cpu_time_gauge, gpu_time_gauge
// - draw_calls_gauge, triangles_gauge
// - gpu_main_pass_gauge

// Update metrics
metrics.update_basic(16.67, 12.5, 4.2);
```

### 2. Adding Custom Metrics (Programmatically)

```rust
fn setup_custom_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let mut metrics = EngineMetrics::with_default_config();

    // Add game metrics
    metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Number of player deaths")?;
    metrics.register_custom_gauge("player_health", "game", "player.health", "Player health", "percentage")?;
    metrics.register_custom_counter("items_collected", "game", "items.collected", "Items collected")?;

    // Network metrics
    metrics.register_custom_gauge("ping", "network", "latency.ping", "Ping to server", "milliseconds")?;

    // Metrics with labels
    let labels = vec![("level".to_string(), "1".to_string())];
    metrics.register_custom_counter_with_labels("level_attempts", "game", "level.attempts", "Level attempts", labels)?;

    // Update
    metrics.increment_counter("player_deaths", 1);
    metrics.set_gauge("player_health", 85.0);
    
    Ok(())
}
```

### 3. JSON-Based Configuration

Create a `custom_metrics.json` file:

```json
{
  "metrics": {
    "player_score": {
      "namespace": "game",
      "name": "player.score",
      "description": "Current player score",
      "metric_type": "gauge",
      "unit": "points"
    },
    "enemy_kills": {
      "namespace": "game", 
      "name": "enemies.killed",
      "description": "Enemies killed",
      "metric_type": "counter"
    },
    "network_latency": {
      "namespace": "network",
      "name": "latency.current",
      "description": "Current network latency",
      "metric_type": "gauge",
      "unit": "milliseconds"
    }
  }
}
```

Then load it:

```rust
fn load_custom_config() -> Result<(), Box<dyn std::error::Error>> {
    // Start with default metrics
    let mut metrics = EngineMetrics::with_default_config();
    // Extend with custom metrics
    metrics.extend_from_file("custom_metrics.json")?;

    // Or create from scratch with custom config
    let mut custom_metrics = EngineMetrics::new();
    custom_metrics.initialize_from_file("my_game_metrics.json")?;
    
    Ok(())
}
```

## Business Wrapper

For more natural usage in a game, you can create a wrapper:

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

pub struct GameMetrics {
    engine_metrics: EngineMetrics,
}

impl GameMetrics {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut metrics = EngineMetrics::with_default_config();
        
        // Add game-specific metrics
        metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Player deaths")?;
        metrics.register_custom_gauge("player_score", "game", "player.score", "Score", "points")?;
        
        Ok(Self { engine_metrics: metrics })
    }
    
    // High-level API
    pub fn player_died(&self) {
        self.engine_metrics.increment_counter("player_deaths", 1);
    }
    
    pub fn update_score(&self, score: u32) {
        self.engine_metrics.set_gauge("player_score", score as f64);
    }
    
    // Access to engine metrics
    pub fn engine_metrics(&self) -> &EngineMetrics {
        &self.engine_metrics
    }
}
```

## Recommended Namespaces

| Namespace | Description | Examples |
|-----------|-------------|----------|
| `engine` | Engine metrics | `engine.frames.total`, `engine.memory.usage_mb` |
| `game` | Game logic | `game.player.score`, `game.enemies.killed` |
| `network` | Network | `network.latency.ping`, `network.packets.sent` |
| `audio` | Audio | `audio.volume.master`, `audio.effects.active` |
| `render` | Rendering | `render.draw_calls`, `render.triangles` |
| `input` | Input | `input.keys.pressed`, `input.mouse.clicks` |
| `ai` | Artificial intelligence | `ai.pathfinding.calculations`, `ai.decisions.made` |

## Monitoring and Debug

```rust
// Get metrics snapshot
let snapshot = metrics.get_metrics_snapshot();
for metric in snapshot {
    println!("{}", metric);
}

// Log all metrics
metrics.log_metrics_summary();
```

## Migration from Old API

If you were using the old API with separate `MetricsRegistry`:

**Before:**
```rust
let registry = MetricsRegistry::new();
let mut engine_metrics = EngineMetrics::new();
engine_metrics.initialize(&registry)?;
```

**Now:**
```rust
let engine_metrics = EngineMetrics::with_default_config();
// Registry is now internal to EngineMetrics
```

## Future Extensibility

The system is designed to be easily extended:

- **New backends**: Implement the `MetricsBackend` trait
- **New metric types**: Add variants to `MetricValue`
- **Export**: Add exporters (Prometheus, InfluxDB, etc.)
- **Aggregation**: Add computed or aggregated metrics

For more examples, see `docs/metrics/examples.md`.
