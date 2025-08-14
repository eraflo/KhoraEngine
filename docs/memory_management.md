# Memory Management Documentation

The memory management system in KhoraEngine provides comprehensive allocation tracking and monitoring through a custom global allocator that wraps the system allocator.

## Table of Contents

1. [Overview](#overview)
2. [SaaTrackingAllocator](#saatrackingallocator)
3. [Memory Monitoring](#memory-monitoring)
4. [Integration with Engine](#integration-with-engine)
5. [Performance Considerations](#performance-considerations)  
6. [Usage Examples](#usage-examples)

## Overview

The memory management system consists of:

- **`SaaTrackingAllocator`**: Global allocator that tracks all heap allocations
- **Memory statistics collection**: Real-time tracking of allocation patterns
- **Integration with engine monitoring**: Memory data fed into performance systems

### Key Features

- **Zero-overhead tracking**: Minimal performance impact on allocations
- **Thread-safe**: Safe to use across multiple threads
- **Detailed statistics**: Peak usage, current usage, allocation counts

## SaaTrackingAllocator

The `SaaTrackingAllocator` is a global allocator wrapper that tracks all heap allocations while delegating actual allocation to the system allocator.

### Structure

```rust
pub struct SaaTrackingAllocator {
    inner: System,
    // Basic tracking
    total_allocated: AtomicUsize,
    peak_allocated: AtomicU64,
    
    // Extended statistics (NEW)
    total_allocations: AtomicU64,
    total_deallocations: AtomicU64,
    total_reallocations: AtomicU64,
    bytes_allocated_lifetime: AtomicU64,
    bytes_deallocated_lifetime: AtomicU64,
    
    // Size categorization (NEW)
    large_allocations: AtomicU64,      // ≥ 1MB
    large_allocation_bytes: AtomicU64,
    small_allocations: AtomicU64,      // < 1MB
    small_allocation_bytes: AtomicU64,
    medium_allocations: AtomicU64,     // 1KB-1MB
    medium_allocation_bytes: AtomicU64,
}
```

### Extended Statistics

The allocator now tracks comprehensive allocation patterns:

- **Allocation Counters**: Total allocations, deallocations, reallocations
- **Lifetime Tracking**: Total bytes allocated/deallocated throughout application lifetime
- **Size Categories**: Automatic classification (small <1KB, medium 1KB-1MB, large ≥1MB)
- **Performance Metrics**: Fragmentation ratio, allocation efficiency, average sizes

### Global Usage

The allocator is installed globally using the `#[global_allocator]` attribute:

```rust
use khora_engine_core::memory::SaaTrackingAllocator;

#[global_allocator]
static ALLOCATOR: SaaTrackingAllocator = SaaTrackingAllocator::new();
```

This enables automatic tracking of all heap allocations throughout the application.

### Statistics Available

```rust
pub struct AllocationStats {
    pub total_allocated_bytes: usize,    // Current total allocated
    pub peak_allocated_bytes: u64,       // Peak allocation reached
    pub allocation_count: usize,         // Total allocations made
    pub deallocation_count: usize,       // Total deallocations made
    pub net_allocations: isize,          // allocation_count - deallocation_count
}
```

## Memory Monitoring

### Getting Statistics

```rust
use khora_engine_core::memory::get_allocation_stats;

let stats = get_allocation_stats();
println!("Current memory usage: {} bytes", stats.total_allocated_bytes);
println!("Peak memory usage: {} bytes", stats.peak_allocated_bytes);
println!("Active allocations: {}", stats.net_allocations);
```

### Thread Safety

All memory tracking operations are thread-safe and use atomic operations for minimal overhead:

```rust
impl GlobalAlloc for SaaTrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = self.inner.alloc(layout);
        if !ptr.is_null() {
            let size = layout.size();
            self.total_allocated.fetch_add(size, Ordering::Relaxed);
            self.allocation_count.fetch_add(1, Ordering::Relaxed);
            
            // Track peak usage
            let current = self.total_allocated.load(Ordering::Relaxed);
            self.peak_allocated.fetch_max(current as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        self.total_allocated.fetch_sub(size, Ordering::Relaxed);
        self.deallocation_count.fetch_add(1, Ordering::Relaxed);
        self.inner.dealloc(ptr, layout);
    }
}
```

### Performance Impact

The tracking adds minimal overhead:
- **Allocation overhead**: ~2-3 atomic operations per allocation
- **Memory overhead**: ~32 bytes for the allocator state
- **Runtime cost**: <1% in typical applications

## Integration with Engine

### Specialized Memory Monitor Integration

The memory tracking is now integrated through the specialized `MemoryResourceMonitor` system:

```rust
use khora_engine_core::core::resource_monitors::MemoryResourceMonitor;
use khora_engine_core::core::monitoring::{MemoryMonitor, ResourceMonitor};

// Engine automatically creates and registers memory monitor
let memory_monitor = Arc::new(MemoryResourceMonitor::new("SystemRAM".to_string()));
register_resource_monitor(memory_monitor.clone());
```

#### Automatic Updates

The memory monitor automatically updates with each engine frame:

```rust
impl ResourceMonitor for MemoryResourceMonitor {
    fn update(&self) {
        self.update_memory_stats(); // Called every frame
    }
}

impl MemoryMonitor for MemoryResourceMonitor {
    fn update_memory_stats(&self) {
        let current_usage = get_currently_allocated_bytes();
        let extended_stats = get_extended_memory_stats(); // NEW: Get comprehensive statistics
        
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
        
        // Create comprehensive report with all extended statistics
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
            *peak = current_usage;
        }
        
        // Calculate allocation delta and create detailed report
        let report = MemoryReport {
            current_usage_bytes: current_usage,
            peak_usage_bytes: *peak,
            allocation_delta_bytes: /* calculated delta */,
            sample_count: /* incremented count */,
        };
        
        // Store report for retrieval
        let mut last_report = self.last_report.lock().unwrap();
        *last_report = Some(report);
    }
}
```

## Advanced Memory Analysis

### Extended Statistics Overview

The memory tracking system now provides comprehensive analytics beyond basic allocation tracking:

```rust
pub struct ExtendedMemoryStats {
    // Current state
    pub current_allocated_bytes: usize,
    pub peak_allocated_bytes: u64,
    
    // Allocation counters
    pub total_allocations: u64,
    pub total_deallocations: u64,
    pub total_reallocations: u64,
    pub net_allocations: i64,
    
    // Lifetime totals
    pub bytes_allocated_lifetime: u64,
    pub bytes_deallocated_lifetime: u64,
    pub bytes_net_lifetime: i64,
    
    // Size category tracking
    pub large_allocations: u64,        // ≥ 1MB
    pub large_allocation_bytes: u64,
    pub small_allocations: u64,        // < 1KB
    pub small_allocation_bytes: u64,
    pub medium_allocations: u64,       // 1KB - 1MB
    pub medium_allocation_bytes: u64,
    
    // Calculated metrics
    pub average_allocation_size: f64,
    pub fragmentation_ratio: f64,
    pub allocation_efficiency: f64,
}
```

### Performance Diagnostics

#### Fragmentation Analysis

```rust
if let Some(report) = memory_monitor.get_memory_report() {
    match report.fragmentation_status() {
        "Low" => println!("✅ Memory fragmentation is healthy"),
        "Moderate" => println!("⚠️ Moderate fragmentation detected"), 
        "High" => println!("🔶 High fragmentation - consider optimization"),
        "Critical" => println!("🔴 Critical fragmentation - immediate attention required"),
        _ => {}
    }
    
    println!("Fragmentation ratio: {:.3}", report.fragmentation_ratio);
}
```

#### Allocation Pattern Analysis

```rust
// Analyze allocation patterns
if let Some(report) = memory_monitor.get_memory_report() {
    println!("📊 Allocation Analysis:");
    
    // Size distribution
    let large_percentage = report.large_allocation_percentage();
    if large_percentage > 30.0 {
        println!("⚠️ High percentage of large allocations: {:.1}%", large_percentage);
    }
    
    // Memory turnover
    let turnover = report.memory_turnover_rate();
    if turnover > 10.0 {
        println!("🔄 High memory turnover: {:.2} ops/sample", turnover);
    }
    
    // Efficiency metrics
    let efficiency = report.memory_utilization_efficiency();
    if efficiency < 95.0 {
        println!("📉 Memory efficiency below optimal: {:.1}%", efficiency);
    }
}
```

#### Reallocation Tracking

```rust
// Monitor reallocation patterns
let extended_stats = get_extended_memory_stats();
if extended_stats.total_reallocations > 0 {
    let realloc_ratio = extended_stats.total_reallocations as f64 / 
                       extended_stats.total_allocations as f64;
    
    if realloc_ratio > 0.1 {
        println!("⚠️ High reallocation ratio: {:.1}%", realloc_ratio * 100.0);
        println!("Consider pre-sizing collections to reduce reallocations");
    }
}
```

#### Metrics System Integration

All memory data is automatically exposed through the metrics system:

```rust
// Automatic metrics updates (handled by engine)
engine_metrics.update_gauge("engine.memory.usage_mb", memory_usage_mb)?;
engine_metrics.update_gauge("engine.memory.peak_mb", memory_peak_mb)?;
engine_metrics.update_counter("engine.memory.total_allocations", total_allocations)?;
engine_metrics.update_gauge("engine.memory.fragmentation_ratio", fragmentation_ratio)?;
engine_metrics.update_gauge("engine.memory.allocation_efficiency", efficiency)?;
```

#### Accessing Memory Data

Applications can access memory data through multiple interfaces:

```rust
// Via resource monitor registry
if let Some(memory_monitor) = get_registered_monitor::<MemoryResourceMonitor>("SystemRAM") {
    if let Some(report) = memory_monitor.get_memory_report() {
        println!("Current: {} KB, Peak: {} KB, Delta: {} KB", 
            report.current_usage_bytes / 1024,
            report.peak_usage_bytes / 1024,
            report.allocation_delta_bytes / 1024);
    }
}

// Via direct allocator stats (legacy interface)
let stats = get_allocation_stats();
println!("Total allocated: {} MB", stats.total_allocated_bytes as f64 / 1_048_576.0);
```

### Engine Statistics

The memory statistics are integrated into the engine's monitoring system:

```rust
// In engine monitoring
let memory_stats = get_allocation_stats();
log::debug!(
    "Memory: {:.2} MB allocated, {:.2} MB peak, {} active allocs",
    memory_stats.total_allocated_bytes as f64 / 1_048_576.0,
    memory_stats.peak_allocated_bytes as f64 / 1_048_576.0,
    memory_stats.net_allocations
);
```

### Periodic Reporting

The engine can be configured to report memory statistics periodically:

```rust
impl Engine {
    fn log_periodic_stats(&self) {
        if self.frame_count % 300 == 0 { // Every 5 seconds at 60fps
            let stats = get_allocation_stats();
            log::info!(
                "Frame {}: Memory {:.1}MB (peak {:.1}MB), Allocs: {} net",
                self.frame_count,
                stats.total_allocated_bytes as f32 / 1_048_576.0,
                stats.peak_allocated_bytes as f32 / 1_048_576.0,
                stats.net_allocations
            );
        }
    }
}
```

### Resource Monitoring Integration

The memory tracking integrates with other resource monitoring systems:

```rust
use khora_engine_core::core::monitoring::ResourceMonitor;

impl ResourceMonitor for MemoryMonitor {
    fn get_resource_usage(&self) -> HashMap<String, f64> {
        let stats = get_allocation_stats();
        let mut usage = HashMap::new();
        
        usage.insert("memory_allocated_mb".to_string(), 
                    stats.total_allocated_bytes as f64 / 1_048_576.0);
        usage.insert("memory_peak_mb".to_string(), 
                    stats.peak_allocated_bytes as f64 / 1_048_576.0);
        usage.insert("active_allocations".to_string(), 
                    stats.net_allocations as f64);
        
        usage
    }
}
```

## Performance Considerations

### Optimization Strategies

1. **Pool Allocations**: Reduce allocation frequency with object pools
2. **Arena Allocators**: Use arena allocators for temporary allocations
3. **Memory Budgets**: Set and enforce memory budgets per subsystem
4. **Streaming**: Load/unload assets based on memory pressure

### Memory Patterns

The tracking system helps identify problematic allocation patterns:

```rust
// Memory leak detection
fn check_memory_leaks(previous_stats: AllocationStats) {
    let current_stats = get_allocation_stats();
    let net_change = current_stats.net_allocations - previous_stats.net_allocations;
    
    if net_change > LEAK_THRESHOLD {
        log::warn!("Potential memory leak: {} new net allocations", net_change);
    }
}

// Memory pressure detection
fn check_memory_pressure() {
    let stats = get_allocation_stats();
    let current_mb = stats.total_allocated_bytes as f32 / 1_048_576.0;
    
    if current_mb > MEMORY_BUDGET_MB {
        log::warn!("Memory pressure: {:.1}MB > {:.1}MB budget", current_mb, MEMORY_BUDGET_MB);
        // Trigger cleanup or quality reduction
    }
}
```

### Best Practices

1. **Monitor Regularly**: Check memory stats periodically, not every frame
2. **Set Budgets**: Define memory budgets for different subsystems
3. **Handle Pressure**: Implement graceful degradation under memory pressure
4. **Profile Allocations**: Use the stats to identify allocation hotspots

## Usage Examples

### Basic Memory Monitoring

```rust
use khora_engine_core::memory::{SaaTrackingAllocator, get_allocation_stats};

// The allocator is installed globally, so all allocations are tracked
#[global_allocator]
static ALLOCATOR: SaaTrackingAllocator = SaaTrackingAllocator::new();

fn main() {
    // Any allocation is automatically tracked
    let data = vec![0u8; 1024 * 1024]; // 1MB allocation
    
    let stats = get_allocation_stats();
    println!("Allocated: {} bytes", stats.total_allocated_bytes);
    
    drop(data); // Deallocation is also tracked
    
    let stats = get_allocation_stats();
    println!("After drop: {} bytes", stats.total_allocated_bytes);
}
```

### Memory Budget Enforcement

```rust
struct MemoryBudgetManager {
    budget_mb: f32,
    warning_threshold: f32,
}

impl MemoryBudgetManager {
    fn check_budget(&self) -> MemoryStatus {
        let stats = get_allocation_stats();
        let current_mb = stats.total_allocated_bytes as f32 / 1_048_576.0;
        
        if current_mb > self.budget_mb {
            MemoryStatus::OverBudget(current_mb - self.budget_mb)
        } else if current_mb > self.warning_threshold {
            MemoryStatus::Warning(current_mb)
        } else {
            MemoryStatus::Ok(current_mb)
        }
    }
    
    fn handle_memory_pressure(&self) {
        match self.check_budget() {
            MemoryStatus::OverBudget(overage) => {
                log::error!("Memory over budget by {:.1}MB", overage);
                // Trigger aggressive cleanup
                self.emergency_cleanup();
            }
            MemoryStatus::Warning(usage) => {
                log::warn!("Memory usage at {:.1}MB, approaching budget", usage);
                // Trigger gentle cleanup
                self.gentle_cleanup();
            }
            MemoryStatus::Ok(usage) => {
                log::debug!("Memory usage: {:.1}MB", usage);
            }
        }
    }
}
```

### Allocation Pattern Analysis

```rust
struct AllocationTracker {
    samples: Vec<AllocationStats>,
    sample_interval: Duration,
    last_sample: Instant,
}

impl AllocationTracker {
    fn update(&mut self) {
        if self.last_sample.elapsed() >= self.sample_interval {
            let stats = get_allocation_stats();
            self.samples.push(stats);
            self.last_sample = Instant::now();
            
            // Keep only recent samples
            if self.samples.len() > 1000 {
                self.samples.remove(0);
            }
        }
    }
    
    fn analyze_trend(&self) -> AllocationTrend {
        if self.samples.len() < 2 {
            return AllocationTrend::Unknown;
        }
        
        let first = &self.samples[0];
        let last = &self.samples[self.samples.len() - 1];
        
        let growth = last.total_allocated_bytes as i64 - first.total_allocated_bytes as i64;
        let growth_rate = growth as f64 / self.samples.len() as f64;
        
        if growth_rate > 1024.0 { // 1KB per sample
            AllocationTrend::Increasing(growth_rate)
        } else if growth_rate < -1024.0 {
            AllocationTrend::Decreasing(growth_rate)
        } else {
            AllocationTrend::Stable(growth_rate)
        }
    }
}
```

### Integration with Rendering System

```rust
// Example: Texture loading with memory awareness
impl TextureManager {
    fn load_texture(&mut self, path: &str) -> Result<TextureId, Error> {
        // Check memory before loading
        let stats = get_allocation_stats();
        let current_mb = stats.total_allocated_bytes as f32 / 1_048_576.0;
        
        if current_mb > self.texture_memory_budget {
            // Unload least recently used textures
            self.cleanup_lru_textures()?;
        }
        
        // Load texture
        let texture_data = std::fs::read(path)?;
        let texture_id = self.create_texture(texture_data)?;
        
        // Log memory usage after loading
        let new_stats = get_allocation_stats();
        log::debug!(
            "Loaded texture {}: memory {:.1}MB -> {:.1}MB",
            path,
            current_mb,
            new_stats.total_allocated_bytes as f32 / 1_048_576.0
        );
        
        Ok(texture_id)
    }
}
```

This memory management system provides comprehensive allocation tracking and monitoring within KhoraEngine, enabling performance optimization and resource management based on actual memory usage patterns.

For implementation details, see the source code in `khora_engine_core/src/memory/`.
