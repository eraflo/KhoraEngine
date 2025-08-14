# System RAM Tracking Integration

## Overview

The `MemoryResourceMonitor` integrates the existing `SaaTrackingAllocator` with the unified resource monitoring system, providing centralized memory tracking alongside GPU and VRAM monitors.

## Architecture

```
MemoryResourceMonitor → SaaTrackingAllocator → get_currently_allocated_bytes()
        ↓
  ResourceMonitor trait (core interface)
        ↓  
  MemoryMonitor trait (specialized interface)
```

## Key Features

- **Real-time tracking**: Uses `SaaTrackingAllocator` for current heap allocations
- **Peak usage monitoring**: Tracks maximum memory usage with manual reset capability  
- **Specialized interface**: Implements both `ResourceMonitor` (general) and `MemoryMonitor` (specialized) traits
- **Thread-safe**: Uses `Mutex` for concurrent access
- **Automatic registration**: Integrated into engine initialization

## Usage

### Automatic (Recommended)
```rust
let engine = Engine::new(); // Automatically registers memory monitor

// Access via registry
let monitors = get_registered_monitors();
let memory_monitor = monitors.iter()
    .find(|m| m.resource_type() == MonitoredResourceType::SystemRam)?;

let usage = memory_monitor.get_usage_report();
```

### Manual
```rust
let monitor = Arc::new(MemoryResourceMonitor::new("SystemRAM".to_string()));
register_resource_monitor(monitor.clone());

// Use specialized interface
monitor.update_memory_stats();
let memory_report = monitor.get_memory_report()?;
```

## Traits

### ResourceMonitor (Core Interface)
- `monitor_id()` - Unique identifier
- `resource_type()` - Returns `MonitoredResourceType::SystemRam`
- `get_usage_report()` - General resource usage data

### MemoryMonitor (Specialized Interface)  
- `get_memory_report()` - Detailed memory statistics
- `update_memory_stats()` - Trigger new sample collection
- `reset_peak_usage()` - Reset peak tracking to current usage

## Data Types

### ResourceUsageReport
```rust
pub struct ResourceUsageReport {
    pub current_bytes: u64,
    pub peak_bytes: Option<u64>, 
    pub total_capacity_bytes: Option<u64>, // None for system memory
}
```

### MemoryReport
```rust
pub struct MemoryReport {
    pub current_usage_bytes: usize,
    pub peak_usage_bytes: usize,
    pub allocation_delta_bytes: usize,
    pub sample_count: u64,
}
```

## Limitations

- Only tracks allocations through `SaaTrackingAllocator`
- System memory capacity not available (`total_capacity_bytes` is `None`)
- May return 0 in test environments where custom allocator is inactive
