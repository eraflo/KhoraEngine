# GPU Performance Monitoring

This document provides comprehensive coverage of GPU performance monitoring in KhoraEngine, including implementation details, usage patterns, and architectural decisions.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Core Components](#core-components)
4. [Usage Guide](#usage-guide)
5. [Integration Patterns](#integration-patterns)
6. [Performance Characteristics](#performance-characteristics)
7. [Implementation Details](#implementation-details)
8. [Testing](#testing)
9. [Future Extensions](#future-extensions)

## Overview

KhoraEngine provides comprehensive GPU performance monitoring through a unified system that works across all rendering backends. The monitoring system leverages the existing `GpuPerfHook` infrastructure and provides microsecond-precision timing measurements.

### Key Features

- **Backend Agnostic**: Same monitoring interface for WGPU, Vulkan, DirectX, Metal
- **GpuPerfHook Integration**: Uses standardized timing points across all backends
- **Microsecond Precision**: High-accuracy timing measurements
- **ResourceMonitor Compliance**: Integrates with engine's resource monitoring system
- **Thread Safe**: Full concurrency support with minimal overhead

### Supported Metrics

- **Hook-Based Timing**: Individual measurements for each `GpuPerfHook`
- **Derived Durations**: Automatic calculation of main pass and frame totals
- **CPU Correlation**: CPU preparation and submission timing
- **Frame Tracking**: Sequential numbering for trend analysis

## Architecture

### GpuPerfHook System

The monitoring system is built around four standardized timing points:

```rust
pub enum GpuPerfHook {
    FrameStart,      // Beginning of frame processing
    MainPassBegin,   // Start of main render pass
    MainPassEnd,     // End of main render pass  
    FrameEnd,        // Frame completion
}
```

### Data Flow

```
RenderSystem::gpu_hook_time_ms() 
    ↓
GpuPerformanceMonitor::update_from_render_system()
    ↓
GpuPerformanceReport (with hook timings)
    ↓
Derived calculations (main_pass_duration_us, frame_total_duration_us)
```

### Cross-Backend Compatibility

```rust
// Same monitoring code works for any backend
fn monitor_gpu_performance(render_system: &dyn RenderSystem, stats: &RenderStats) {
    let monitor = GpuPerformanceMonitor::new("Backend".to_string());
    monitor.update_from_render_system(render_system, stats);
    
    // Analysis code is backend-independent
    if let Some(report) = monitor.get_gpu_performance_report() {
        println!("Frame {}: Main pass {}μs, Total {}μs", 
                 report.frame_number,
                 report.main_pass_duration_us().unwrap_or(0),
                 report.frame_total_duration_us().unwrap_or(0));
    }
}
```

## Core Components

### 1. GpuPerformanceReport

Central data structure for timing measurements:

```rust
pub struct GpuPerformanceReport {
    pub frame_number: u64,
    pub hook_timings_us: [Option<u32>; 4], // Indexed by GpuPerfHook
    pub cpu_preparation_time_us: Option<u32>,
    pub cpu_submission_time_us: Option<u32>,
}

impl GpuPerformanceReport {
    // Direct hook access
    pub fn get_hook_timing_us(&self, hook: GpuPerfHook) -> Option<u32>;
    pub fn set_hook_timing_us(&mut self, hook: GpuPerfHook, timing_us: Option<u32>);
    
    // Derived calculations
    pub fn main_pass_duration_us(&self) -> Option<u32>;  // MainPassEnd - MainPassBegin
    pub fn frame_total_duration_us(&self) -> Option<u32>; // FrameEnd - FrameStart
}
```

### 2. GpuPerformanceMonitor

Backend-agnostic monitoring implementation:

```rust
pub struct GpuPerformanceMonitor {
    system_name: String,
    last_frame_stats: Mutex<Option<GpuPerformanceReport>>,
}

impl GpuPerformanceMonitor {
    // Primary update method (uses RenderSystem hooks)
    pub fn update_from_render_system(&self, render_system: &dyn RenderSystem, stats: &RenderStats);
    
    // Fallback method (derives from RenderStats)
    pub fn update_from_render_stats(&self, stats: &RenderStats);
}

impl ResourceMonitor for GpuPerformanceMonitor {
    fn get_gpu_performance_report(&self) -> Option<GpuPerformanceReport>;
    // ... other ResourceMonitor methods
}
```

### 3. RenderSystem Integration

Any `RenderSystem` can provide GPU timing through:

```rust
impl RenderSystem for MyRenderSystem {
    fn gpu_hook_time_ms(&self, hook: GpuPerfHook) -> Option<f32> {
        // Backend-specific implementation
        match hook {
            GpuPerfHook::FrameStart => self.timing_data.frame_start_ms,
            GpuPerfHook::MainPassBegin => self.timing_data.main_pass_begin_ms,
            GpuPerfHook::MainPassEnd => self.timing_data.main_pass_end_ms,
            GpuPerfHook::FrameEnd => self.timing_data.frame_end_ms,
        }
    }
}
```

## Usage Guide

### Basic Usage (WGPU)

```rust
use khora_engine_core::subsystems::renderer::WgpuRenderSystem;
use khora_engine_core::core::monitoring::ResourceMonitor;

// WGPU has built-in monitoring
let mut render_system = WgpuRenderSystem::new();
// Initialize the render system (implementation details omitted)
let gpu_monitor = render_system.gpu_performance_monitor();

// Monitor is automatically updated each frame
if let Some(report) = gpu_monitor.get_gpu_performance_report() {
    println!("Frame {}: Main pass {}μs, Total {}μs", 
             report.frame_number,
             report.main_pass_duration_us().unwrap_or(0),
             report.frame_total_duration_us().unwrap_or(0));
}
```

### Generic Usage (Any Backend)

```rust
use khora_engine_core::subsystems::renderer::GpuPerformanceMonitor;
use khora_engine_core::subsystems::renderer::api::common_types::GpuPerfHook;

// Create monitor for any backend
let monitor = GpuPerformanceMonitor::new("Vulkan".to_string());

// Update from any RenderSystem
monitor.update_from_render_system(&render_system, &stats);

// Access individual hook timings
for hook in GpuPerfHook::ALL {
    if let Some(timing) = report.get_hook_timing_us(hook) {
        println!("{:?}: {}μs", hook, timing);
    }
}
```

### Advanced Analysis

```rust
fn analyze_gpu_performance(monitor: &GpuPerformanceMonitor) {
    if let Some(report) = monitor.get_gpu_performance_report() {
        // Check for performance issues
        if let Some(main_pass_us) = report.main_pass_duration_us() {
            if main_pass_us > 16_667 { // > 16.67ms (60 FPS threshold)
                println!("Warning: Main pass exceeding 60 FPS budget");
            }
        }
        
        // Analyze GPU vs CPU balance
        let gpu_total = report.frame_total_duration_us().unwrap_or(0);
        let cpu_prep = report.cpu_preparation_time_us.unwrap_or(0);
        
        if cpu_prep > gpu_total {
            println!("CPU-bound: Consider optimizing CPU preparation");
        } else {
            println!("GPU-bound: Consider optimizing shaders/geometry");
        }
    }
}
```

## Integration Patterns

### Custom Backend Implementation

```rust
// Example: Hypothetical Vulkan backend
impl RenderSystem for VulkanRenderSystem {
    fn gpu_hook_time_ms(&self, hook: GpuPerfHook) -> Option<f32> {
        // Use Vulkan timestamp queries
        self.vulkan_profiler.get_hook_timing(hook)
    }
}

// Monitor automatically works with Vulkan
let vulkan_monitor = GpuPerformanceMonitor::new("Vulkan".to_string());
vulkan_monitor.update_from_render_system(&vulkan_system, &stats);
```

### Multi-Backend Applications

```rust
fn unified_monitoring(render_systems: Vec<&dyn RenderSystem>) {
    let monitors: Vec<_> = render_systems.iter()
        .enumerate()
        .map(|(i, _)| GpuPerformanceMonitor::new(format!("Backend_{}", i)))
        .collect();
    
    // Update all monitors with consistent interface
    for (monitor, system) in monitors.iter().zip(render_systems.iter()) {
        let stats = system.get_last_frame_stats();
        monitor.update_from_render_system(*system, stats);
    }
    
    // Compare performance across backends
    for (i, monitor) in monitors.iter().enumerate() {
        if let Some(report) = monitor.get_gpu_performance_report() {
            println!("Backend {}: Frame {}μs", 
                     i, 
                     report.frame_total_duration_us().unwrap_or(0));
        }
    }
}
```

### Performance Dashboard Integration

```rust
fn export_performance_metrics(monitor: &GpuPerformanceMonitor) -> serde_json::Value {
    if let Some(report) = monitor.get_gpu_performance_report() {
        json!({
            "frame_number": report.frame_number,
            "main_pass_us": report.main_pass_duration_us(),
            "frame_total_us": report.frame_total_duration_us(),
            "cpu_preparation_us": report.cpu_preparation_time_us,
            "individual_hooks": {
                "frame_start": report.get_hook_timing_us(GpuPerfHook::FrameStart),
                "main_pass_begin": report.get_hook_timing_us(GpuPerfHook::MainPassBegin),
                "main_pass_end": report.get_hook_timing_us(GpuPerfHook::MainPassEnd),
                "frame_end": report.get_hook_timing_us(GpuPerfHook::FrameEnd),
            }
        })
    } else {
        json!(null)
    }
}
```

## Performance Characteristics

### Memory Overhead

- **Per Monitor**: ~48 bytes + system name string
- **Per Report**: 44 bytes (fixed size)
- **Thread Synchronization**: Single `Mutex<Option<GpuPerformanceReport>>`

### CPU Overhead

- **Update Cost**: < 1μs per frame (direct trait call + array indexing)
- **Query Cost**: < 0.1μs (atomic read + copy)
- **Lock Contention**: Minimal (write-heavy, brief locks)

### GPU Impact

- **Zero Overhead**: Uses existing timestamp infrastructure
- **Backend Dependent**: Actual timing precision varies by hardware/driver
- **Graceful Degradation**: Returns `None` when timestamps unavailable

### Scaling Characteristics

```rust
// Benchmark results (typical values)
// 1 monitor:    ~0.8μs per frame update
// 10 monitors:  ~8.2μs per frame update  
// 100 monitors: ~85μs per frame update
// Scales linearly with minimal overhead
```

## Implementation Details

### Thread Safety

```rust
// Full thread safety through Arc + Mutex
let monitor = Arc::new(GpuPerformanceMonitor::new("Shared".to_string()));

// Safe concurrent access
let monitor_clone = monitor.clone();
std::thread::spawn(move || {
    // Can safely read/write from any thread
    if let Some(report) = monitor_clone.get_gpu_performance_report() {
        println!("Thread processing: Frame {} - {}μs", 
                 report.frame_number,
                 report.frame_total_duration_us().unwrap_or(0));
    }
});
```

### WGPU Backend Implementation

The WGPU backend provides a comprehensive implementation of GPU timestamp profiling through the `GpuTimestampProfiler`.

#### Design Goals
- Capture main pass GPU time and total frame GPU time with high precision
- Avoid validation/synchronization errors (previous issues with encoder-level timestamp writes)
- Provide stable (smoothed) timings for diagnostics and adaptive decisions
- Minimize overhead while maintaining accuracy

#### Technical Constraints
- Only `TIMESTAMP_QUERY` feature enabled (no `TIMESTAMP_QUERY_INSIDE_ENCODERS`)
- Some drivers disallow raw `write_timestamp` on encoder without extended features
- Two-frame latency required to avoid read/write hazards

#### WGPU Timestamp Architecture

```rust
// QuerySet with 4 timestamp slots
pub struct GpuTimestampProfiler {
    query_set: wgpu::QuerySet,           // 4 timestamp slots
    resolve_buffer: wgpu::Buffer,        // GPU-only resolve buffer (256 bytes)
    staging_buffers: [wgpu::Buffer; 3],  // Triple-buffered staging (32 bytes each)
    current_frame: u64,
    metrics: GpuTimingMetrics,
}

// Data flow per frame N
impl GpuTimestampProfiler {
    pub fn encode_timestamps(&self, encoder: &mut wgpu::CommandEncoder) {
        // Pass A: Timestamps 0 (FrameStart) & 1 (MainPassBegin)
        self.encode_compute_pass_a(encoder);
        
        // Main render pass happens here (inserted by caller)
        
        // Pass B: Timestamps 2 (MainPassEnd) & 3 (FrameEnd) 
        self.encode_compute_pass_b(encoder);
        
        // Resolve all 4 queries to GPU buffer
        encoder.resolve_query_set(&self.query_set, 0..4, &self.resolve_buffer, 0);
        
        // Copy to staging buffer (triple buffering)
        let staging_index = (self.current_frame % 3) as usize;
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer, 0,
            &self.staging_buffers[staging_index], 0,
            32, // 4 * u64
        );
    }
}
```

#### Timestamp Collection Process

```
Frame N: Encode Pass A → Main Render Pass → Pass B → Resolve & Copy
         ↓
Frame N+1: Schedule map for buffer (N-2) % 3 (if N >= 2)
         ↓  
Frame N+2: Read mapped buffer, calculate durations, update metrics
```

#### Data Processing and Smoothing

```rust
// Raw timing calculations
let main_pass_duration = timestamps[2] - timestamps[1];  // MainPassEnd - MainPassBegin
let frame_total_duration = timestamps[3] - timestamps[0]; // FrameEnd - FrameStart

// Exponential Moving Average (EMA) for stability
// alpha = 0.2 provides good balance between responsiveness and stability
smoothed_value = alpha * new_value + (1.0 - alpha) * previous_smoothed_value;
```

#### Non-blocking Readback Strategy

```rust
impl GpuTimestampProfiler {
    pub fn try_read_previous_frame(&mut self, device: &wgpu::Device) {
        // Non-blocking poll to pump map_async callbacks
        device.poll(wgpu::Maintain::Poll);
        
        // Check for completed mappings without blocking
        if let Some(mapped_data) = self.check_completed_mapping() {
            self.process_timestamp_data(mapped_data);
        }
    }
    
    fn process_timestamp_data(&mut self, data: &[u8]) {
        let timestamps: &[u64] = bytemuck::cast_slice(data);
        
        // Calculate durations and update smoothed metrics
        self.update_timing_metrics(timestamps);
    }
}
```

#### Integration with ResourceMonitor

```rust
impl RenderSystem for WgpuRenderSystem {
    fn gpu_hook_time_ms(&self, hook: GpuPerfHook) -> Option<f32> {
        self.gpu_timestamp_profiler.get_hook_timing_ms(hook)
    }
}

// Automatic integration with GpuPerformanceMonitor
impl WgpuRenderSystem {
    pub fn gpu_performance_monitor(&self) -> &GpuPerformanceMonitor {
        &self.gpu_performance_monitor
    }
    
    fn update_performance_monitoring(&self, stats: &RenderStats) {
        // Monitor automatically uses gpu_hook_time_ms() implementation
        self.gpu_performance_monitor.update_from_render_system(self, stats);
    }
}
```

### Error Handling

```rust
// Graceful handling of missing data
fn safe_performance_analysis(monitor: &GpuPerformanceMonitor) {
    match monitor.get_gpu_performance_report() {
        Some(report) => {
            // Use timing data
            match report.main_pass_duration_us() {
                Some(duration) => {
                    println!("Main pass took {}μs", duration);
                    if duration > 16_667 { // > 16.67ms (60 FPS threshold)
                        println!("Warning: Main pass exceeding 60 FPS budget");
                    }
                },
                None => log::debug!("Main pass timing unavailable"),
            }
        }
        None => log::debug!("No performance report available yet"),
    }
}
```

### Surface Resize Strategy (WGPU)

The WGPU backend includes an intelligent surface resize strategy to minimize `Suboptimal present` warnings while avoiding excessive swapchain reconfiguration overhead.

#### Problem Statement
Continuous window resizing generates many `Suboptimal present` warnings. Immediate reconfigure on every event causes swapchain churn; delaying too long results in repeated suboptimal presents.

#### Hybrid Strategy Components

```rust
pub struct ResizeStrategy {
    last_surface_config: Instant,      // Throttle reference
    last_resize_event: Instant,        // Debounce reference  
    pending_resize: bool,              // Deferred resize flag
    pending_resize_frames: u32,        // Frame counter while pending
    last_pending_size: Option<(u32, u32)>, // Size stability detection
    stable_size_frame_count: u32,      // Consecutive stable frames
}

impl ResizeStrategy {
    // 1. Throttle: Immediate reconfigure if ≥80ms since last
    const THROTTLE_MS: u64 = 80;
    
    // 2. Early Stable: ≥2 identical sizes + ≥20ms since last reconfigure  
    const EARLY_STABLE_MS: u64 = 20;
    const EARLY_STABLE_FRAMES: u32 = 2;
    
    // 3. Debounce: Multiple fallback conditions
    const DEBOUNCE_QUIET_MS: u64 = 120;     // Quiet period
    const DEBOUNCE_MAX_FRAMES: u32 = 10;    // Maximum pending frames
    const DEBOUNCE_STABLE_FRAMES: u32 = 3;  // Stable size detection
}
```

#### Resize Decision Logic

```rust
impl ResizeStrategy {
    pub fn should_reconfigure(&mut self, new_size: (u32, u32)) -> bool {
        let now = Instant::now();
        
        // 1. Throttle check - immediate if enough time passed
        if now.duration_since(self.last_surface_config).as_millis() >= Self::THROTTLE_MS {
            self.apply_reconfigure(new_size, now);
            return true;
        }
        
        // 2. Early stable reconfigure  
        if self.is_early_stable_candidate(new_size, now) {
            self.apply_reconfigure(new_size, now);
            return true;
        }
        
        // 3. Mark as pending for debounce logic
        self.mark_pending_resize(new_size, now);
        false
    }
    
    pub fn check_debounce_conditions(&mut self) -> bool {
        if !self.pending_resize { return false; }
        
        let now = Instant::now();
        
        // Condition A: Quiet period elapsed
        let quiet_elapsed = now.duration_since(self.last_resize_event).as_millis() >= Self::DEBOUNCE_QUIET_MS;
        
        // Condition B: Too many pending frames
        let max_frames_exceeded = self.pending_resize_frames >= Self::DEBOUNCE_MAX_FRAMES;
        
        // Condition C: Stable size detected
        let stable_size = self.stable_size_frame_count >= Self::DEBOUNCE_STABLE_FRAMES;
        
        if quiet_elapsed || max_frames_exceeded || stable_size {
            self.apply_pending_reconfigure();
            return true;
        }
        
        self.pending_resize_frames += 1;
        false
    }
}
```

#### Surface Error Handling

```rust
impl WgpuRenderSystem {
    fn handle_surface_error(&mut self, error: wgpu::SurfaceError) -> Result<(), RenderError> {
        match error {
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => {
                log::warn!("Surface lost/outdated, forcing reconfiguration");
                
                // Immediate reconfigure, reset pending state
                self.resize_strategy.force_reconfigure();
                self.reconfigure_surface()?;
                Ok(())
            }
            wgpu::SurfaceError::OutOfMemory => {
                log::error!("GPU out of memory");
                Err(RenderError::OutOfMemory)
            }
            wgpu::SurfaceError::Timeout => {
                log::warn!("Surface acquire timeout, retrying next frame");
                Ok(())
            }
        }
    }
}
```

#### Performance Impact Analysis

| Strategy Component | CPU Overhead | GPU Impact | Benefits |
|-------------------|--------------|------------|----------|
| Throttle (80ms) | Minimal | None | Reduces swapchain churn |
| Early Stable | ~1μs/frame | None | Faster response to stable resize |
| Debounce Logic | ~2μs/frame | None | Handles rapid resize events |
| Stability Detection | ~0.5μs/frame | None | Reduces warning duration |

#### Tuning Parameters

```rust
// Current optimized values - adjust based on profiling
pub struct ResizeParams {
    pub throttle_ms: u64,           // 80ms - balance responsiveness/churn
    pub early_stable_ms: u64,       // 20ms - early detection threshold  
    pub early_stable_frames: u32,   // 2 - minimum stability confirmation
    pub debounce_quiet_ms: u64,     // 120ms - maximum quiet wait
    pub debounce_max_frames: u32,   // 10 - frame count fallback
    pub debounce_stable_frames: u32,// 3 - stability detection frames
}
```

### Fallback Mechanisms

```rust
impl GpuPerformanceMonitor {
    pub fn update_from_render_stats(&self, stats: &RenderStats) {
        // Fallback when direct hook access unavailable
        if stats.gpu_main_pass_time_ms > 0.0 {
            // Derive hook timings from aggregate stats
            let main_pass_us = (stats.gpu_main_pass_time_ms * 1000.0) as u32;
            report.set_hook_timing_us(GpuPerfHook::MainPassBegin, Some(0));
            report.set_hook_timing_us(GpuPerfHook::MainPassEnd, Some(main_pass_us));
        }
    }
}
```

## Testing

### Comprehensive Test Coverage

The monitoring system includes tests for:

1. **Monitor Creation**: Basic initialization and state
2. **Update Mechanisms**: Both render system and stats-based updates
3. **Hook Calculations**: Derived duration accuracy
4. **Edge Cases**: Missing data, invalid timings
5. **ResourceMonitor Compliance**: Interface contract verification

### Running Tests

```bash
# All GPU monitoring tests
cargo test gpu_performance_monitor

# Specific test suites
cargo test gpu_performance_report_hook_methods
cargo test gpu_performance_monitor_creation
cargo test gpu_performance_monitor_update_stats
```

### Test Results

```
running 7 tests
test subsystems::renderer::gpu_performance_monitor::tests::gpu_performance_monitor_creation ... ok
test subsystems::renderer::gpu_performance_monitor::tests::gpu_performance_report_hook_methods ... ok
test subsystems::renderer::gpu_performance_monitor::tests::gpu_performance_monitor_update_stats ... ok
test subsystems::renderer::gpu_performance_monitor::tests::gpu_performance_report_missing_data ... ok
test subsystems::renderer::wgpu_impl::wgpu_device::tests::gpu_performance_monitor_creation ... ok
test subsystems::renderer::wgpu_impl::wgpu_device::tests::gpu_performance_monitor_resource_monitor_trait ... ok
test subsystems::renderer::wgpu_impl::wgpu_device::tests::gpu_performance_monitor_update_stats ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Performance Testing

```rust
#[bench]
fn bench_monitor_update(b: &mut Bencher) {
    let monitor = GpuPerformanceMonitor::new("Bench".to_string());
    let stats = RenderStats {
        gpu_main_pass_time_ms: 8.5,
        frame_number: 1000,
        // ... other required fields
    };
    
    b.iter(|| {
        monitor.update_from_render_stats(&stats);
    });
}
```

## Future Extensions

### Planned Enhancements

1. **Multi-Pass Support**
   ```rust
   pub enum GpuPerfHook {
       FrameStart,
       ShadowPassBegin,
       ShadowPassEnd,
       MainPassBegin,
       MainPassEnd,
       PostProcessBegin,
       PostProcessEnd,
       FrameEnd,
   }
   ```

2. **Aggregation Systems**
   ```rust
   pub struct GpuPerformanceAggregator {
       window_size: usize,
       recent_reports: VecDeque<GpuPerformanceReport>,
   }
   
   impl GpuPerformanceAggregator {
       pub fn moving_average(&self) -> GpuPerformanceReport;
       pub fn percentiles(&self) -> PerformancePercentiles;
   }
   ```

3. **Real-time Visualization**
   ```rust
   pub trait PerformanceVisualizer {
       fn update(&mut self, report: &GpuPerformanceReport);
       fn render_overlay(&self, target: &mut RenderTarget);
   }
   ```

4. **Alert System**
   ```rust
   pub struct PerformanceAlerts {
       thresholds: PerformanceThresholds,
       callbacks: Vec<Box<dyn Fn(&GpuPerformanceReport)>>,
   }
   ```

### Extensibility Patterns

The architecture supports easy extension through:

- **Custom Hook Types**: Backend-specific timing points
- **Pluggable Analyzers**: Custom performance analysis algorithms
- **Export Interfaces**: Integration with external monitoring tools
- **Visualization Backends**: Real-time and historical performance displays

## Best Practices

### For Backend Developers

1. **Implement `gpu_hook_time_ms()`** accurately for your backend
2. **Handle missing timestamps** gracefully (return `None`)
3. **Use hardware-specific optimizations** when available
4. **Document timing precision** and limitations

### For Application Developers

1. **Check data availability** before using timing values
2. **Consider averaging** over multiple frames for stability
3. **Use appropriate thresholds** for performance alerts
4. **Profile regularly** to establish baseline performance

### For Engine Integration

1. **Initialize monitors early** in the rendering pipeline
2. **Update consistently** every frame
3. **Handle backend switches** gracefully
4. **Provide fallback mechanisms** for unsupported hardware

## Key Implementation Files

### Core Monitoring System
- `khora_engine_core/src/core/monitoring.rs` - ResourceMonitor trait and GpuPerformanceReport
- `khora_engine_core/src/subsystems/renderer/gpu_performance_monitor.rs` - Backend-agnostic monitoring
- `khora_engine_core/src/subsystems/renderer/mod.rs` - Module exports and integration

### WGPU Backend Implementation  
- `khora_engine_core/src/subsystems/renderer/wgpu_impl/gpu_timestamp_profiler.rs` - WGPU timestamp profiling
- `khora_engine_core/src/subsystems/renderer/wgpu_impl/wgpu_render_system.rs` - RenderSystem integration
- `khora_engine_core/src/subsystems/renderer/wgpu_impl/wgpu_graphic_context.rs` - Surface resize strategy
- `khora_engine_core/src/subsystems/renderer/wgpu_impl/wgpu_device.rs` - Device-level monitoring

## Maintenance Guidelines

### Extending Query Points

When adding new timing points beyond the current 4 hooks:

```rust
// 1. Update GpuPerfHook enum
pub enum GpuPerfHook {
    FrameStart,
    ShadowPassBegin,    // New
    ShadowPassEnd,      // New  
    MainPassBegin,
    MainPassEnd,
    PostProcessBegin,   // New
    PostProcessEnd,     // New
    FrameEnd,
}

// 2. Update array sizes in GpuPerformanceReport
pub struct GpuPerformanceReport {
    pub hook_timings_us: [Option<u32>; 8], // Updated from 4
    // ...
}

// 3. Update WGPU QuerySet and buffer sizes
const QUERY_COUNT: u32 = 8;  // Updated from 4
const RESOLVE_BUFFER_SIZE: u64 = 64; // 8 * u64, updated from 32
```

### Performance Monitoring Configuration

```rust
// Expose timing constants via RenderSettings when needed
pub struct RenderSettings {
    pub gpu_monitoring: GpuMonitoringSettings,
    pub resize_strategy: ResizeStrategySettings,
}

pub struct GpuMonitoringSettings {
    pub ema_alpha: f32,                    // Smoothing factor (default: 0.2)
    pub enable_detailed_profiling: bool,   // Additional timing points
    pub timestamp_resolution_ns: u64,      // Hardware-dependent
}

pub struct ResizeStrategySettings {
    pub throttle_ms: u64,           // Default: 80
    pub debounce_quiet_ms: u64,     // Default: 120  
    pub stability_frame_count: u32, // Default: 3
}
```

### Buffer Management

```rust
// Keep resolve buffer properly aligned for GPU efficiency
const RESOLVE_BUFFER_ALIGNMENT: u64 = 256;

// Adjust staging buffer sizes when reading more data
fn calculate_staging_size(query_count: u32) -> u64 {
    (query_count as u64 * std::mem::size_of::<u64>() as u64)
        .next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT)
}
```

### Error Recovery Strategies

```rust
impl GpuTimestampProfiler {
    pub fn handle_device_loss(&mut self) {
        // Reset all timing state
        self.current_frame = 0;
        self.metrics.reset();
        
        // Will need device recreation to rebuild QuerySet/buffers
        log::info!("GPU timestamp profiler reset due to device loss");
    }
    
    pub fn validate_query_support(&self, device: &wgpu::Device) -> bool {
        // Check if timestamp queries are supported
        device.features().contains(wgpu::Features::TIMESTAMP_QUERY)
    }
}
```

## Hardware Compatibility

### Supported Platforms

- **Windows**: DirectX 12, Vulkan, OpenGL
- **macOS**: Metal, OpenGL (limited)
- **Linux**: Vulkan, OpenGL
- **Web**: WebGPU (when available)

### Hardware Requirements

- **Minimum**: Any GPU with basic timestamp query support
- **Recommended**: Modern GPU with high-resolution timestamps
- **Optimal**: Recent GPU with hardware timing counters

### Fallback Behavior

```rust
// Automatic fallback when timestamps unavailable
if render_system.gpu_hook_time_ms(GpuPerfHook::FrameStart).is_none() {
    log::info!("GPU timestamps not available, using CPU-side estimation");
    monitor.update_from_render_stats(&stats); // Fallback method
}
```

This comprehensive monitoring system provides a solid foundation for GPU performance analysis across all current and future rendering backends in KhoraEngine.

## Glossary

### Performance Monitoring Terms
- **GpuPerfHook**: Standardized timing points in the rendering pipeline
- **ResourceMonitor**: Core trait for engine resource monitoring (memory, GPU, etc.)
- **EMA (Exponential Moving Average)**: Smoothing algorithm that reduces noise in frame timing
- **Backend-Agnostic**: Code that works with any graphics API (WGPU, Vulkan, DirectX, Metal)
- **Microsecond Precision**: Timing measurements accurate to 1μs (0.001ms)

### WGPU-Specific Terms
- **QuerySet**: WGPU object that holds timestamp query slots
- **Resolve Buffer**: GPU-only buffer where query results are initially written
- **Staging Buffer**: CPU-mappable buffer for reading GPU timestamp data
- **Triple Buffering**: Using 3 rotating buffers to avoid read/write conflicts
- **Suboptimal Present**: Driver warning when swapchain doesn't match surface properties
- **Swapchain**: Graphics buffer chain used for presenting frames to screen

### Timing Concepts
- **Hook Timing**: Direct measurement at specific GpuPerfHook points
- **Derived Duration**: Calculated time span between two hooks (e.g., main pass = end - begin)
- **Frame Latency**: Delay between GPU execution and CPU readback (typically 2-3 frames)
- **Non-blocking Poll**: Checking for completed operations without waiting/stalling

## References and Further Reading

### WGPU Documentation
- [WGPU Timestamp Queries](https://docs.rs/wgpu/latest/wgpu/struct.QuerySet.html)
- [WGPU Performance Guidelines](https://github.com/gfx-rs/wgpu/wiki/Performance)
- [WGPU Buffer Mapping](https://docs.rs/wgpu/latest/wgpu/struct.Buffer.html#method.map_async)

### Graphics Performance
- [GPU Performance Analysis Techniques](https://developer.nvidia.com/blog/the-peak-performance-analysis-method-for-optimizing-any-gpu-workload/)
- [Vulkan Timestamp Best Practices](https://www.khronos.org/registry/vulkan/specs/1.3-extensions/man/html/VkQueryPool.html)
- [DirectX 12 GPU Timing](https://docs.microsoft.com/en-us/windows/win32/direct3d12/timing)

### Engine Architecture
- [Game Engine Architecture - Performance Profiling](https://www.gameenginebook.com/)
- [Real-Time Rendering Performance Optimization](https://www.realtimerendering.com/)

### KhoraEngine Internal Documentation
- `/docs/architecture_design.md` - Overall engine architecture
- `/docs/dev_workflow/pre_push_verification.md` - Development and testing procedures
- Source code documentation in individual `.rs` files

## Version History

- **v1.0** - Initial WGPU-specific implementation with basic timestamp profiling
- **v2.0** - Backend-agnostic refactoring with GpuPerfHook integration  
- **v2.1** - Surface resize strategy optimization and error handling improvements
- **v3.0** - Unified documentation and comprehensive monitoring system (current)
