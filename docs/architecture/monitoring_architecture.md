# Resource Monitoring Architecture

## Overview

KhoraEngine uses a specialized trait-based architecture for resource monitoring that provides type-safe, efficient, and extensible performance tracking across the engine.

## Architecture Design

### Core Trait Hierarchy

```
ResourceMonitor (base trait)
├── MemoryMonitor (memory-specific operations)
├── GpuMonitor (GPU performance operations)  
└── VramMonitor (VRAM-specific operations)
```

### Base ResourceMonitor Trait

All monitors implement the core `ResourceMonitor` trait:

```rust
pub trait ResourceMonitor: Send + Sync + Debug + 'static {
    /// Unique identifier for this monitor instance
    fn monitor_id(&self) -> Cow<'static, str>;

    /// Type of resource being monitored
    fn resource_type(&self) -> MonitoredResourceType;

    /// Get general resource usage information
    fn get_usage_report(&self) -> ResourceUsageReport;

    /// Update the monitor's internal state/statistics
    /// Default implementation does nothing for monitors that don't need updates
    fn update(&self) {
        // Default: no-op
    }
}
```

## Specialized Monitor Traits

### MemoryMonitor

Extends `ResourceMonitor` for comprehensive system memory tracking:

```rust
pub trait MemoryMonitor: ResourceMonitor {
    /// Get detailed memory report with extended statistics
    fn get_memory_report(&self) -> Option<MemoryReport>;
    
    /// Update memory statistics from allocator
    fn update_memory_stats(&self);
    
    /// Reset peak usage tracking
    fn reset_peak_usage(&self);
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryReport {
    // Basic statistics
    pub current_usage_bytes: usize,
    pub peak_usage_bytes: usize,
    pub allocation_delta_bytes: usize,
    pub sample_count: u64,
    
    // Extended statistics
    pub total_allocations: u64,
    pub total_deallocations: u64,
    pub total_reallocations: u64,
    pub bytes_allocated_lifetime: u64,
    pub bytes_deallocated_lifetime: u64,
    pub large_allocations: u64,      // ≥ 1MB
    pub large_allocation_bytes: u64,
    pub small_allocations: u64,      // < 1MB
    pub small_allocation_bytes: u64,
    
    // Calculated metrics
    pub fragmentation_ratio: f64,
    pub allocation_efficiency: f64,
    pub average_allocation_size: f64,
}
```

#### MemoryReport Analysis Methods

The `MemoryReport` provides rich analysis capabilities:

```rust
impl MemoryReport {
    // Size conversions
    pub fn current_usage_mb(&self) -> f64;
    pub fn peak_usage_mb(&self) -> f64;
    pub fn allocation_delta_kb(&self) -> f64;
    
    // Performance analysis
    pub fn memory_turnover_rate(&self) -> f64;
    pub fn large_allocation_percentage(&self) -> f64;
    pub fn memory_utilization_efficiency(&self) -> f64;
    pub fn average_allocation_size_mb(&self) -> f64;
    
    // Diagnostic methods
    pub fn fragmentation_status(&self) -> &'static str; // "Low"/"Moderate"/"High"/"Critical"
}
```

### GpuMonitor

Extends `ResourceMonitor` for GPU performance tracking:

```rust
pub trait GpuMonitor: ResourceMonitor {
    /// Get GPU performance report
    fn get_gpu_report(&self) -> Option<GpuReport>;
}

#[derive(Debug, Clone, Copy)]
pub struct GpuReport {
    pub frame_number: u64,
    pub hook_timings_us: [Option<u32>; 4], // FrameStart, MainPassBegin, MainPassEnd, FrameEnd
    pub cpu_preparation_time_us: Option<u32>,
    pub cpu_submission_time_us: Option<u32>,
}
```

### VramMonitor

Extends `ResourceMonitor` for VRAM tracking:

```rust
pub trait VramMonitor: ResourceMonitor {
    /// Get VRAM usage report
    fn get_vram_report(&self) -> Option<VramReport>;
}

#[derive(Debug, Clone, Copy)]
pub struct VramReport {
    pub current_usage_mb: f32,
    pub peak_usage_mb: f32,
    pub capacity_mb: Option<f32>,
}
```

## Monitor Implementations

### MemoryResourceMonitor

Integrates with the `SaaTrackingAllocator` for real-time memory tracking:

```rust
pub struct MemoryResourceMonitor {
    monitor_id: String,
    last_report: Mutex<Option<MemoryReport>>,
    peak_usage_bytes: Mutex<usize>,
    last_allocation_bytes: Mutex<usize>,
    sample_count: Mutex<u64>,
}

impl ResourceMonitor for MemoryResourceMonitor {
    fn update(&self) {
        self.update_memory_stats(); // Called every frame with extended stats
    }
}

impl MemoryMonitor for MemoryResourceMonitor {
    fn update_memory_stats(&self) {
        let current_usage = get_currently_allocated_bytes();
        let extended_stats = get_extended_memory_stats(); // NEW: Extended statistics
        
        // Create comprehensive report with all metrics
        let report = MemoryReport {
            // Basic metrics
            current_usage_bytes: current_usage,
            peak_usage_bytes: peak_usage,
            allocation_delta_bytes: delta,
            sample_count: count,
            
            // Extended statistics from SaaTrackingAllocator
            total_allocations: extended_stats.total_allocations,
            total_deallocations: extended_stats.total_deallocations,
            total_reallocations: extended_stats.total_reallocations,
            bytes_allocated_lifetime: extended_stats.bytes_allocated_lifetime,
            bytes_deallocated_lifetime: extended_stats.bytes_deallocated_lifetime,
            large_allocations: extended_stats.large_allocations,
            large_allocation_bytes: extended_stats.large_allocation_bytes,
            small_allocations: extended_stats.small_allocations,
            small_allocation_bytes: extended_stats.small_allocation_bytes,
            fragmentation_ratio: extended_stats.fragmentation_ratio,
            allocation_efficiency: extended_stats.allocation_efficiency,
            average_allocation_size: extended_stats.average_allocation_size,
        };
    }
}
```

**Key Features:**
- **Comprehensive Statistics**: Beyond basic allocation tracking
- **Size Categorization**: Automatic small/large allocation classification (1MB threshold)
- **Performance Analytics**: Fragmentation detection, efficiency metrics, turnover rates
- **Lifetime Tracking**: Total bytes allocated/deallocated throughout application lifetime
- **Real-time Metrics**: Fragmentation ratio, allocation efficiency, average sizes
- **Diagnostic Tools**: Automatic fragmentation status classification
- **Thread-safe Updates**: Frame-by-frame collection of extended statistics

### GpuMonitor

Tracks GPU performance from render system statistics:

```rust
pub struct GpuMonitor {
    system_name: String,
    last_frame_stats: Mutex<Option<GpuReport>>,
}

impl ResourceMonitor for GpuMonitor {
    fn update(&self) {
        // GPU updates handled by render system via update_from_frame_stats()
    }
}
```

**Key Features:**
- Receives data from render system
- Tracks frame timings and CPU/GPU synchronization
- Hook-based timeline profiling

### VramResourceMonitor

Monitors video memory through graphics device abstraction:

```rust
pub struct VramResourceMonitor {
    vram_provider: Weak<dyn VramProvider>,
    monitor_id: String,
}

pub trait VramProvider: Send + Sync + Debug {
    fn get_vram_usage_mb(&self) -> f32;
    fn get_vram_peak_mb(&self) -> f32;
    fn get_vram_capacity_mb(&self) -> Option<f32>;
}
```

**Key Features:**
- Decoupled from specific graphics implementations
- Weak reference to prevent circular dependencies
- Automatic updates through provider interface

## Registry System

### Monitor Registration

All monitors are managed through a centralized registry:

```rust
// During engine initialization
fn initialize_resource_monitoring() {
    // Register memory monitor
    let memory_monitor = Arc::new(MemoryResourceMonitor::new("SystemRAM".to_string()));
    register_resource_monitor(memory_monitor.clone());
    
    // Register GPU monitor
    let gpu_monitor = Arc::new(GpuMonitor::new("WgpuGPU".to_string()));
    register_resource_monitor(gpu_monitor.clone());
    
    // Register VRAM monitor (when render system is available)
    if let Some(vram_provider) = get_vram_provider() {
        let vram_monitor = Arc::new(VramResourceMonitor::new(
            Arc::downgrade(&vram_provider),
            "WgpuVRAM".to_string()
        ));
        register_resource_monitor(vram_monitor);
    }
}
```

### Monitor Access

Retrieve monitors safely from the registry:

```rust
// Get typed monitor reference
if let Some(memory_monitor) = get_registered_monitor::<MemoryResourceMonitor>("SystemRAM") {
    if let Some(report) = memory_monitor.get_memory_report() {
        println!("Memory usage: {} KB", report.current_usage_bytes / 1024);
    }
}

// Iterate all monitors
for monitor in get_registered_monitors() {
    let usage = monitor.get_usage_report();
    println!("{}: {} bytes", monitor.monitor_id(), usage.current_bytes);
    
    // Update monitor state
    monitor.update();
}
```

## Update Patterns

### Memory Monitor Updates

```rust
impl MemoryMonitor for MemoryResourceMonitor {
    fn update_memory_stats(&self) {
        let current_usage = get_currently_allocated_bytes();
        let extended_stats = get_extended_memory_stats(); // NEW: Get comprehensive stats

        // Update peak tracking
        let mut peak = self.peak_usage_bytes.lock().unwrap();
        if current_usage > *peak {
            *peak = current_usage;
        }

        // Calculate allocation delta
        let mut last_alloc = self.last_allocation_bytes.lock().unwrap();
        let allocation_delta = current_usage.saturating_sub(*last_alloc);
        *last_alloc = current_usage;

        // Update sample count
        let mut count = self.sample_count.lock().unwrap();
        *count += 1;

        // Create comprehensive report with extended statistics
        let report = MemoryReport {
            // Basic metrics
            current_usage_bytes: current_usage,
            peak_usage_bytes: *peak,
            allocation_delta_bytes: allocation_delta,
            sample_count: *count,
            
            // Extended statistics from allocator
            total_allocations: extended_stats.total_allocations,
            total_deallocations: extended_stats.total_deallocations,
            total_reallocations: extended_stats.total_reallocations,
            bytes_allocated_lifetime: extended_stats.bytes_allocated_lifetime,
            bytes_deallocated_lifetime: extended_stats.bytes_deallocated_lifetime,
            large_allocations: extended_stats.large_allocations,
            large_allocation_bytes: extended_stats.large_allocation_bytes,
            small_allocations: extended_stats.small_allocations,
            small_allocation_bytes: extended_stats.small_allocation_bytes,
            fragmentation_ratio: extended_stats.fragmentation_ratio,
            allocation_efficiency: extended_stats.allocation_efficiency,
            average_allocation_size: extended_stats.average_allocation_size,
        };

        let mut last_report = self.last_report.lock().unwrap();
        *last_report = Some(report);
    }
}
```

### Engine Integration

The engine automatically calls `update()` on all registered monitors:

```rust
impl Engine {
    fn update_resource_metrics(&mut self) {
        for monitor in get_registered_monitors() {
            monitor.update(); // Calls specialized update for each monitor type
        }
    }
}
```

## Metrics Integration

All monitor data is automatically integrated into the metrics system:

```rust
// Engine metrics update cycle
self.engine_metrics.update_gauge(
    "engine.memory.usage_mb",
    memory_usage_bytes as f64 / (1024.0 * 1024.0)
)?;

self.engine_metrics.update_gauge(
    "engine.memory.vram_usage_mb", 
    vram_usage_mb as f64
)?;

self.engine_metrics.update_gauge(
    "engine.performance.gpu_time_ms",
    gpu_frame_time_ms as f64
)?;
```

## Benefits of This Architecture

### Type Safety
- Each monitor type has specific methods for its domain
- Compile-time guarantees for monitor capabilities
- No runtime type casting required

### Performance
- Minimal overhead from trait dispatch
- Efficient update patterns per monitor type
- Lock-free reads where possible

### Extensibility
- Easy to add new monitor types
- Trait inheritance allows specialization
- Registry system supports dynamic monitor management

### Clean Separation
- Engine doesn't directly couple to specific monitors
- Registry pattern allows loose coupling
- Each monitor handles its own update logic

## Usage Examples

### Basic Monitor Query
```rust
// Check memory usage
if let Some(memory_monitor) = get_registered_monitor::<MemoryResourceMonitor>("SystemRAM") {
    let report = memory_monitor.get_memory_report().unwrap();
    println!("Current: {} KB, Peak: {} KB", 
        report.current_usage_bytes / 1024,
        report.peak_usage_bytes / 1024);
}
```

### Extended Memory Analysis
```rust
// Advanced memory diagnostics with extended statistics
if let Some(memory_monitor) = get_registered_monitor::<MemoryMonitor>("SystemMemory") {
    if let Some(report) = memory_monitor.get_memory_report() {
        println!("📊 Memory Analysis:");
        println!("  Current: {:.2} MB, Peak: {:.2} MB", 
                report.current_usage_mb(), report.peak_usage_mb());
        
        // Allocation statistics
        println!("  Allocations: {} total, {} deallocations, {} reallocations",
                report.total_allocations, report.total_deallocations, report.total_reallocations);
        
        // Size distribution analysis
        println!("  Large allocations (≥1MB): {} ({:.1}%)",
                report.large_allocations, report.large_allocation_percentage());
        
        // Performance metrics
        println!("  Average allocation: {:.2} MB", report.average_allocation_size_mb());
        println!("  Memory turnover: {:.2} ops/sample", report.memory_turnover_rate());
        println!("  Efficiency: {:.1}%", report.memory_utilization_efficiency());
        
        // Fragmentation analysis
        println!("  Fragmentation: {} ({:.3})", 
                report.fragmentation_status(), report.fragmentation_ratio);
        
        // Lifetime statistics
        println!("  Lifetime allocated: {:.2} MB", 
                report.bytes_allocated_lifetime as f64 / (1024.0 * 1024.0));
        println!("  Lifetime deallocated: {:.2} MB", 
                report.bytes_deallocated_lifetime as f64 / (1024.0 * 1024.0));
    }
}
```

### Monitor Health Check
```rust
// Check all monitor health
for monitor in get_registered_monitors() {
    let usage = monitor.get_usage_report();
    if let Some(capacity) = usage.total_capacity_bytes {
        let usage_percent = (usage.current_bytes as f64 / capacity as f64) * 100.0;
        if usage_percent > 80.0 {
            warn!("Monitor {} at {}% capacity", monitor.monitor_id(), usage_percent);
        }
    }
}
```

### Performance Analysis
```rust
// Get GPU performance data
if let Some(gpu_monitor) = get_registered_monitor::<GpuMonitor>("WgpuGPU") {
    if let Some(report) = gpu_monitor.get_gpu_report() {
        if let Some(frame_time) = report.frame_total_duration_us() {
            println!("GPU frame time: {} ms", frame_time as f64 / 1000.0);
        }
    }
}
```
