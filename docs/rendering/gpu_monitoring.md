# GPU Performance Monitoring

This document describes the GPU performance monitoring system in Khora Engine, which provides detailed metrics about GPU rendering performance and resource usage.

## Overview

The GPU monitoring system tracks various performance metrics during rendering operations, including frame timing, draw calls, triangle counts, and VRAM usage estimates. This information is crucial for performance optimization and debugging rendering issues.

## Core Components

### GpuMonitor

The `GpuMonitor` struct is the main component responsible for tracking GPU performance metrics. It implements both the `ResourceMonitor` and `GpuMonitor` traits.

```rust
use khora_engine_core::core::monitoring::GpuMonitor;
use khora_engine_core::core::resource_monitors::RenderStats;

// Create a new GPU monitor
let monitor = GpuMonitor::new("MainGPU".to_string());

// Update with render statistics
let stats = RenderStats {
    frame_number: 1,
    cpu_preparation_time_ms: 2.5,
    cpu_render_submission_time_ms: 0.8,
    gpu_main_pass_time_ms: 12.3,
    gpu_frame_total_time_ms: 16.7,
    draw_calls: 150,
    triangles_rendered: 75000,
    vram_usage_estimate_mb: 512.0,
};

monitor.update_from_frame_stats(&stats);

// Retrieve performance report
if let Some(report) = monitor.get_gpu_report() {
    println!("Frame {}: {}ms total GPU time", 
             report.frame_number, 
             report.frame_total_duration_us().unwrap_or(0) as f32 / 1000.0);
}
```

### GpuReport

The `GpuReport` struct contains detailed performance metrics for a single frame:

```rust
pub struct GpuReport {
    pub frame_number: u64,
    pub cpu_preparation_time_us: Option<u64>,
    pub cpu_submission_time_us: Option<u64>,
    pub gpu_hooks: Vec<GpuHook>,
    pub draw_calls: Option<u32>,
    pub triangles_rendered: Option<u64>,
    pub vram_usage_estimate_mb: Option<f32>,
}
```

#### Key Methods

- `frame_total_duration_us()` - Returns total GPU frame time in microseconds
- `main_pass_duration_us()` - Returns main rendering pass duration in microseconds

### GpuHook

GPU hooks represent timing measurements at different stages of the rendering pipeline:

```rust
pub enum GpuHook {
    FrameStart(u64),
    MainPassStart(u64),
    MainPassEnd(u64),
    FrameEnd(u64),
}
```

## Integration with Rendering System

### WGPU Integration

The GPU monitor integrates seamlessly with the WGPU rendering backend:

```rust
// In your render loop
let render_stats = RenderStats {
    frame_number: current_frame,
    cpu_preparation_time_ms: prep_time,
    cpu_render_submission_time_ms: submit_time,
    gpu_main_pass_time_ms: gpu_main_time,
    gpu_frame_total_time_ms: gpu_total_time,
    draw_calls: total_draw_calls,
    triangles_rendered: total_triangles,
    vram_usage_estimate_mb: estimated_vram_mb,
};

gpu_monitor.update_from_frame_stats(&render_stats);
```

### Performance Analysis

Use the monitoring data for performance analysis:

```rust
if let Some(report) = monitor.get_gpu_report() {
    // Check for performance bottlenecks
    if let Some(total_time) = report.frame_total_duration_us() {
        if total_time > 16667 { // > 16.67ms (60 FPS threshold)
            println!("Warning: Frame time exceeds 60 FPS target");
        }
    }
    
    // Analyze CPU vs GPU bottlenecks
    if let (Some(cpu_prep), Some(gpu_total)) = (
        report.cpu_preparation_time_us,
        report.frame_total_duration_us()
    ) {
        let cpu_percentage = (cpu_prep as f32 / gpu_total as f32) * 100.0;
        println!("CPU preparation is {:.1}% of total frame time", cpu_percentage);
    }
}
```

## Resource Monitor Integration

The `GpuMonitor` also implements the base `ResourceMonitor` trait, allowing it to be used in generic monitoring contexts:

```rust
use khora_engine_core::core::monitoring::ResourceMonitor;

fn log_resource_usage<T: ResourceMonitor>(monitor: &T) {
    let usage = monitor.get_usage_report();
    println!("Resource {}: {} bytes current, {} bytes peak", 
             monitor.monitor_id(),
             usage.current_bytes,
             usage.peak_bytes.unwrap_or(0));
}

// Use with GPU monitor
log_resource_usage(&gpu_monitor);
```

## Best Practices

### Monitoring Frequency

- Update GPU metrics once per frame
- Avoid updating during critical rendering sections
- Consider using a separate thread for metric collection in performance-critical applications

### Performance Impact

The monitoring system is designed to have minimal overhead:
- Metrics are collected asynchronously when possible
- Memory allocations are minimized
- Optional features can be disabled in release builds

### Data Interpretation

- Frame times above 16.67ms indicate dropped frames at 60 FPS
- High CPU preparation times may indicate CPU bottlenecks
- VRAM usage should be monitored to prevent memory exhaustion
- Draw call counts help identify batching opportunities

## Example: Complete Monitoring Setup

```rust
use khora_engine_core::core::monitoring::GpuMonitor;
use khora_engine_core::core::resource_monitors::RenderStats;

fn setup_gpu_monitoring() -> GpuMonitor {
    let monitor = GpuMonitor::new("MainRenderer".to_string());
    
    // In your render loop:
    // monitor.update_from_frame_stats(&render_stats);
    
    monitor
}

fn analyze_performance(monitor: &GpuMonitor) {
    if let Some(report) = monitor.get_gpu_report() {
        println!("=== GPU Performance Report ===");
        println!("Frame: {}", report.frame_number);
        
        if let Some(total_us) = report.frame_total_duration_us() {
            println!("Total GPU time: {:.2}ms", total_us as f32 / 1000.0);
        }
        
        if let Some(main_us) = report.main_pass_duration_us() {
            println!("Main pass time: {:.2}ms", main_us as f32 / 1000.0);
        }
        
        if let Some(draws) = report.draw_calls {
            println!("Draw calls: {}", draws);
        }
        
        if let Some(triangles) = report.triangles_rendered {
            println!("Triangles: {}", triangles);
        }
        
        if let Some(vram_mb) = report.vram_usage_estimate_mb {
            println!("VRAM usage: {:.1} MB", vram_mb);
        }
    }
}
```

This monitoring system provides comprehensive insights into GPU performance, enabling developers to identify bottlenecks and optimize rendering performance effectively.
