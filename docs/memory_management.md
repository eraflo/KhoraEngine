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
    total_allocated: AtomicUsize,
    peak_allocated: AtomicU64,
    allocation_count: AtomicUsize,
    deallocation_count: AtomicUsize,
}
```

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
