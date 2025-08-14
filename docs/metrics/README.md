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
- **Integrated resource monitoring** - Automatic memory, GPU, and VRAM tracking

## Integrated Resource Monitoring

The metrics system now automatically integrates with the comprehensive resource monitoring architecture:

- **Enhanced Memory tracking** via `MemoryResourceMonitor` with extended statistics:
  - Allocation/deallocation/reallocation counters
  - Size categorization (small/medium/large allocations)
  - Fragmentation analysis and efficiency metrics
  - Lifetime tracking and performance analytics
- **GPU performance** via `GpuMonitor` with frame timing data
- **VRAM usage** via `VramResourceMonitor` and graphics device integration

### Extended Memory Metrics

The system now automatically exposes comprehensive memory analytics:

```
engine.memory.usage_mb              - Current memory usage
engine.memory.peak_mb               - Peak memory usage
engine.memory.total_allocations     - Total allocation count
engine.memory.total_deallocations   - Total deallocation count
engine.memory.fragmentation_ratio   - Memory fragmentation (0.0-1.0)
engine.memory.allocation_efficiency - Allocation efficiency percentage
engine.memory.large_allocations     - Count of large allocations (≥1MB)
engine.memory.average_allocation_kb - Average allocation size
```

All resource data is automatically collected and exposed through the metrics system without manual configuration.

## Quick Start

```rust
use khora_engine_core::core::metrics::engine::EngineMetrics;

// Default configuration with integrated monitoring
let metrics = EngineMetrics::with_default_config();

// Custom metrics still supported
let mut metrics = EngineMetrics::with_default_config();
metrics.register_custom_counter("player_deaths", "game", "player.deaths", "Player deaths").unwrap();
metrics.increment_counter("player_deaths", 1);

// Resource metrics are automatic with extended memory analytics
// engine.memory.usage_mb, engine.memory.fragmentation_ratio, 
// engine.memory.allocation_efficiency, engine.performance.gpu_time_ms
// are updated automatically each frame
```

For more details, see the [user guide](user_guide.md) and the [monitoring architecture documentation](../architecture/monitoring_architecture.md).
