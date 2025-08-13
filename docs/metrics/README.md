# Metrics System Documentation

This folder contains all documentation related to the KhoraEngine metrics system.

## Documentation Files

- **[user_guide.md](user_guide.md)** - Complete user guide for the metrics system
- **[architecture.md](architecture.md)** - Technical architecture and design documentation
- **[examples.md](examples.md)** - Practical usage examples
- **[api_reference.md](api_reference.md)** - Complete API reference

## Overview

The KhoraEngine metrics system allows you to:

- Collect real-time performance metrics
- Configure custom metrics via JSON or code
- Organize metrics by logical namespaces
- Export data for monitoring and debugging

## Quick Start

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

// Default configuration
let metrics = EngineMetrics::with_default_config();

// Add custom metrics
let mut metrics = EngineMetrics::with_default_config();
metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Player deaths").unwrap();
metrics.increment_counter("player_deaths", 1);
```

For more details, see the [user guide](user_guide.md).
