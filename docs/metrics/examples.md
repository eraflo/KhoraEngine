# Metrics System Usage Examples

This document presents practical examples of using the metrics system.

## 1. Default Configuration

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

// Create with default engine metrics
let metrics = EngineMetrics::with_default_config();

// Automatically available metrics:
// - frame_counter: frame count
// - fps_gauge: current FPS  
// - memory_gauge: memory usage
// - frame_time_gauge, cpu_time_gauge, gpu_time_gauge
// - draw_calls_gauge, triangles_gauge
// - gpu_main_pass_gauge

// Basic update
metrics.update_basic(16.67, 12.5, 4.2);
```

## 2. Custom Programmatic Metrics

```rust
fn setup_custom_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let mut metrics = EngineMetrics::with_default_config();

    // Game metrics
    metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Number of deaths")?;
    metrics.register_custom_gauge("player_health", "game", "player.health", "Player's current health", "percentage")?;
    metrics.register_custom_counter("items_collected", "game", "items.collected", "Items collected")?;

    // Network metrics
    metrics.register_custom_gauge("ping", "network", "latency.ping", "Server ping", "milliseconds")?;
    metrics.register_custom_counter("packets_sent", "network", "packets.sent", "Packets sent")?;

    // Audio metrics
    metrics.register_custom_gauge("master_volume", "audio", "volume.master", "Master volume", "percent")?;

    // Update
    metrics.increment_counter("player_deaths", 1);
    metrics.set_gauge("player_health", 85.0);
    metrics.increment_counter("items_collected", 3);
    metrics.set_gauge("ping", 45.2);
    
    Ok(())
}
```

## 3. Metrics with Labels

```rust
fn setup_labeled_metrics() -> Result<(), Box<dyn std::error::Error>> {
    let mut metrics = EngineMetrics::with_default_config();

    // Metrics by level and difficulty
    let level_labels = vec![
        ("level".to_string(), "1".to_string()),
        ("difficulty".to_string(), "normal".to_string())
    ];
    metrics.register_custom_counter_with_labels(
        "level_attempts", 
        "game", 
        "level.attempts", 
        "Level attempts", 
        level_labels
    )?;

    // Network metrics by region
    let region_labels = vec![("region".to_string(), "us-east".to_string())];
    metrics.register_custom_gauge_with_labels(
        "regional_latency", 
        "network", 
        "latency.regional", 
        "Regional latency", 
        "ms", 
        region_labels
    )?;

    // Usage
    metrics.increment_counter("level_attempts", 1);
    metrics.set_gauge("regional_latency", 45.7);
    
    Ok(())
}
```

## 4. JSON Configuration

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
      "description": "Network latency",
      "metric_type": "gauge",
      "unit": "milliseconds"
    },
    "player_deaths_by_level": {
      "namespace": "game",
      "name": "player.deaths",
      "description": "Deaths by level",
      "metric_type": "counter",
      "labels": {
        "level": "1",
        "difficulty": "normal"
      }
    }
  }
}
```

Then load it:

```rust
fn setup_from_file() -> Result<(), Box<dyn std::error::Error>> {
    // Method 1: Extend default metrics
    let mut metrics = EngineMetrics::with_default_config();
    metrics.extend_from_file("custom_metrics.json")?;

    // Method 2: Completely custom configuration
    let mut custom_metrics = EngineMetrics::new();
    custom_metrics.initialize_from_file("custom_metrics.json")?;

    // Usage
    metrics.set_gauge("player_score", 1500.0);
    metrics.increment_counter("enemy_kills", 1);
    metrics.set_gauge("network_latency", 42.3);
    
    Ok(())
}
```

## 5. Business Wrapper for a Game

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

pub struct GameMetrics {
    engine_metrics: EngineMetrics,
}

impl GameMetrics {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let mut metrics = EngineMetrics::with_default_config();
        
        // Game-specific metrics
        metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Player deaths")?;
        metrics.register_custom_counter("enemies_killed", "game", "enemies.killed", "Enemies killed")?;
        metrics.register_custom_gauge("player_score", "game", "player.score", "Player score", "points")?;
        metrics.register_custom_gauge("level_progress", "game", "level.progress", "Level progress", "percent")?;
        
        Ok(Self { engine_metrics: metrics })
    }
    
    // High-level API for game events
    pub fn player_died(&self) {
        self.engine_metrics.increment_counter("player_deaths", 1);
    }
    
    pub fn enemy_killed(&self) {
        self.engine_metrics.increment_counter("enemies_killed", 1);
    }
    
    pub fn update_score(&self, score: u32) {
        self.engine_metrics.set_gauge("player_score", score as f64);
    }
    
    pub fn update_level_progress(&self, progress_percent: f32) {
        self.engine_metrics.set_gauge("level_progress", progress_percent as f64);
    }
    
    // Access to engine metrics for performance updates
    pub fn engine_metrics(&self) -> &EngineMetrics {
        &self.engine_metrics
    }
}

// Usage in game
impl GameMetrics {
    pub fn on_player_hit_enemy(&self, damage: u32) {
        if damage > 0 {
            self.enemy_killed();
            self.update_score(self.get_current_score() + damage as u32 * 10);
        }
    }
    
    pub fn on_level_completed(&self, completion_time: f64) {
        self.update_level_progress(100.0);
        // Time bonus
        let time_bonus = (60.0 - completion_time.min(60.0)) * 100.0;
        self.update_score(self.get_current_score() + time_bonus as u32);
    }
    
    fn get_current_score(&self) -> u32 {
        // In practice, you would store the score somewhere
        1000 // Placeholder
    }
}
```

## 6. Monitoring and Debug

```rust
// Get snapshot of all metrics
let snapshot = metrics.get_metrics_snapshot();
for metric in snapshot {
    println!("{}", metric);
}

// Automatically log all metrics
metrics.log_metrics_summary();

// Example output:
// === Engine Metrics Summary ===
//   engine.frames.total: Total frames rendered = Counter(1547)
//   engine.performance.fps: Frames per second = Gauge(60.0)
//   engine.memory.usage_mb: Memory usage in MB = Gauge(45.2)
//   game.player.score: Player score = Gauge(1500.0)
//   game.enemies.killed: Enemies killed = Counter(23)
// === End Metrics Summary ===
```

## 7. Recommended Namespaces

| Namespace | Description | Example Metrics |
|-----------|-------------|-----------------|
| `engine` | Engine metrics | `frames.total`, `memory.usage_mb`, `performance.fps` |
| `game` | Game logic | `player.score`, `enemies.killed`, `level.progress` |
| `network` | Network communications | `latency.ping`, `packets.sent`, `bandwidth.upload` |
| `audio` | Audio system | `volume.master`, `effects.active`, `music.playing` |
| `render` | Rendering system | `draw_calls`, `triangles`, `textures.loaded` |
| `input` | Input management | `keys.pressed`, `mouse.clicks`, `gamepad.connected` |
| `ai` | Artificial intelligence | `pathfinding.calculations`, `decisions.made` |
| `physics` | Physics engine | `collisions.detected`, `bodies.active` |

## 8. Engine Integration

```rust
use khora_engine_core::core::Engine;

// Engine already has integrated metrics system
let mut engine = Engine::new(); // EngineMetrics is automatically created

// Access to engine metrics
let metrics = &engine.engine_metrics;

// Automatic update via engine
let frame_stats = FrameStats {
    fps: 60,
    memory_usage_kib: 1024,
    render_duration_us: 16670,
    gpu_main_pass_ms: 12.5,
    gpu_frame_total_ms: 15.0,
    draw_calls: 150,
    triangles: 50000,
};

engine.update_all_metrics(&frame_stats);
```

These examples showcase the flexibility of the metrics system, allowing both simple usage with default configurations and advanced customization according to specific project needs.
