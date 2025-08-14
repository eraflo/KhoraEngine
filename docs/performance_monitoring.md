# Performance Monitoring and Metrics

# Performance Monitoring Documentation

KhoraEngine includes a comprehensive performance monitoring system designed to provide real-time insights into engine performance and resource usage. This system enables performance analysis and optimization during development and runtime.

## Table of Contents

1. [Overview](#overview)
2. [Core Monitoring Components](#core-monitoring-components)
3. [CPU Performance Monitoring](#cpu-performance-monitoring)
4. [GPU Performance Monitoring](#gpu-performance-monitoring)
5. [Memory Monitoring](#memory-monitoring)
6. [Metrics Collection](#metrics-collection)
7. [Performance Analysis](#performance-analysis)
8. [Usage Examples](#usage-examples)

## Overview

The performance monitoring system tracks multiple aspects of engine performance:

- **CPU Timing**: Frame times, subsystem execution times, specific operation timings
- **GPU Performance**: Frame rendering times, pass timings, GPU resource usage
- **Memory Usage**: Heap allocations, GPU memory (VRAM), peak usage tracking
- **Resource Metrics**: Asset loading times, shader compilation, I/O operations
- **System Metrics**: Overall engine health and performance trends

### Design Goals

1. **Low Overhead**: Minimal performance impact from monitoring itself
2. **Real-time**: Immediate availability of performance data
3. **Comprehensive**: Coverage of all major engine subsystems
4. **Actionable**: Data useful for optimization and development

## Core Monitoring Components

### ResourceMonitor Trait

The base trait for all resource monitoring in the new architecture:

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

### Specialized Monitor Traits

The system uses specialized traits for different resource types:

#### Memory Monitoring
```rust
pub trait MemoryMonitor: ResourceMonitor {
    /// Get memory-specific report
    fn get_memory_report(&self) -> Option<MemoryReport>;
    
    /// Update memory statistics
    fn update_memory_stats(&self);
    
    /// Reset peak usage tracking
    fn reset_peak_usage(&self);
}
```

#### GPU Performance Monitoring
```rust
pub trait GpuMonitor: ResourceMonitor {
    /// Get GPU performance report
    fn get_gpu_report(&self) -> Option<GpuReport>;
}
```

#### VRAM Monitoring
```rust
pub trait VramMonitor: ResourceMonitor {
    /// Get VRAM usage report
    fn get_vram_report(&self) -> Option<VramReport>;
}
```

### Monitor Implementations

#### MemoryResourceMonitor
Tracks system memory usage through integration with the SaaTrackingAllocator:

```rust
pub struct MemoryResourceMonitor {
    monitor_id: String,
    last_report: Mutex<Option<MemoryReport>>,
    peak_usage_bytes: Mutex<usize>,
    last_allocation_bytes: Mutex<usize>,
    sample_count: Mutex<u64>,
}
```

#### GpuMonitor
Tracks GPU performance metrics from the render system:

```rust
pub struct GpuMonitor {
    system_name: String,
    last_frame_stats: Mutex<Option<GpuReport>>,
}
```

#### VramResourceMonitor
Monitors video memory usage through the graphics system:

```rust
pub struct VramResourceMonitor {
    vram_provider: Weak<dyn VramProvider>,
    monitor_id: String,
}
```

### Resource Registry

All monitors are managed through a centralized registry:

```rust
// Register monitors during engine initialization
let memory_monitor = Arc::new(MemoryResourceMonitor::new("SystemRAM".to_string()));
register_resource_monitor(memory_monitor.clone());

let gpu_monitor = Arc::new(GpuMonitor::new("WgpuGPU".to_string()));
register_resource_monitor(gpu_monitor.clone());
```

## CPU Performance Monitoring

### Frame Timing

The engine tracks frame execution time across different phases:

```rust
#[derive(Debug, Clone)]
pub struct FrameTimings {
    pub total_frame_time_ms: f32,
    pub update_time_ms: f32,
    pub render_preparation_time_ms: f32,
    pub render_submission_time_ms: f32,
    pub event_processing_time_ms: f32,
    pub subsystem_times_ms: HashMap<String, f32>,
}
```

### Subsystem Timing

Individual subsystems report their execution times:

```rust
impl RenderSystem {
    fn render_with_timing(&mut self, objects: &[RenderObject]) -> Result<RenderStats, RenderError> {
        let mut timer = Stopwatch::new();
        
        // Time preparation phase
        timer.start();
        self.prepare_render_data(objects)?;
        timer.stop();
        let preparation_time = timer.elapsed_ms();
        
        // Time submission phase
        timer.reset();
        timer.start();
        self.submit_render_commands()?;
        timer.stop();
        let submission_time = timer.elapsed_ms();
        
        Ok(RenderStats {
            cpu_preparation_time_ms: preparation_time,
            cpu_render_submission_time_ms: submission_time,
            // ... other stats
        })
    }
}
```

### Operation-Level Timing

Critical operations can be timed individually:

```rust
// Macro for convenient timing
macro_rules! time_operation {
    ($name:expr, $operation:expr) => {{
        let start = Instant::now();
        let result = $operation;
        let elapsed = start.elapsed().as_secs_f32() * 1000.0;
        log::trace!("Operation '{}' took {:.2}ms", $name, elapsed);
        result
    }};
}

// Usage
let asset = time_operation!("asset_loading", load_asset_from_disk(path))?;
let shader = time_operation!("shader_compilation", compile_shader(source))?;
```

## GPU Performance Monitoring

### Timestamp Queries

The rendering system uses GPU timestamp queries for accurate GPU timing:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuPerfHook {
    FrameStart,      // Beginning of GPU frame work
    MainPassBegin,   // Start of main render pass
    MainPassEnd,     // End of main render pass
    FrameEnd,        // End of GPU frame work
}
```

### GPU Statistics

Comprehensive GPU performance data:

```rust
#[derive(Debug, Clone)]
pub struct GpuStats {
    pub frame_time_ms: f32,           // Total GPU frame time
    pub main_pass_time_ms: f32,       // Main render pass time
    pub vram_allocated_mb: f32,       // Total VRAM allocated
    pub vram_used_mb: f32,           // Currently used VRAM
    pub vram_peak_mb: f32,           // Peak VRAM usage
    pub draw_calls: u32,             // Number of draw calls
    pub triangles_rendered: u64,     // Total triangles in frame
}
```

### Smoothing and Averaging

GPU timings use exponential moving averages for stability:

```rust
pub struct GpuTimestampProfiler {
    raw_frame_time_ms: f32,
    smoothed_frame_time_ms: f32,
    raw_main_pass_time_ms: f32,
    smoothed_main_pass_time_ms: f32,
    ema_alpha: f32, // Exponential moving average factor
}

impl GpuTimestampProfiler {
    fn update_timing(&mut self, raw_frame_time: f32, raw_main_pass_time: f32) {
        self.raw_frame_time_ms = raw_frame_time;
        self.raw_main_pass_time_ms = raw_main_pass_time;
        
        // Apply exponential moving average
        self.smoothed_frame_time_ms = self.ema_alpha * raw_frame_time + 
                                     (1.0 - self.ema_alpha) * self.smoothed_frame_time_ms;
        self.smoothed_main_pass_time_ms = self.ema_alpha * raw_main_pass_time + 
                                         (1.0 - self.ema_alpha) * self.smoothed_main_pass_time_ms;
    }
}
```

## Memory Monitoring

### System Memory Tracking with MemoryResourceMonitor

The new integrated memory monitoring system uses `MemoryResourceMonitor` which connects with the `SaaTrackingAllocator`:

```rust
pub struct MemoryResourceMonitor {
    monitor_id: String,
    last_report: Mutex<Option<MemoryReport>>,
    peak_usage_bytes: Mutex<usize>,
    last_allocation_bytes: Mutex<usize>,
    sample_count: Mutex<u64>,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryReport {
    pub current_usage_bytes: usize,
    pub peak_usage_bytes: usize,
    pub allocation_delta_bytes: usize,
    pub sample_count: u64,
}
```

#### Real-time Memory Updates

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
        
        // Update peak tracking
        let mut peak = self.peak_usage_bytes.lock().unwrap();
        if current_usage > *peak {
            *peak = current_usage;
        }
        
        // Calculate allocation delta and store report
        // ...
    }
}
```

### VRAM Tracking with VramResourceMonitor

GPU memory usage is tracked through the `VramResourceMonitor`:

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

#### Integration with Graphics System

The VRAM monitor connects to the graphics device:

```rust
impl VramProvider for WgpuDevice {
    fn get_vram_usage_mb(&self) -> f32 {
        self.vram_allocated_bytes.load(Ordering::Relaxed) as f32 / (1024.0 * 1024.0)
    }
    
    fn get_vram_peak_mb(&self) -> f32 {
        self.vram_peak_bytes.load(Ordering::Relaxed) as f32 / (1024.0 * 1024.0)
    }
}
```

### Memory Metrics Integration

All memory data is automatically integrated into the metrics system:

```rust
// Engine automatically updates memory metrics each frame
engine_metrics.update_gauge("engine.memory.usage_mb", memory_usage_mb)?;
engine_metrics.update_gauge("engine.memory.vram_usage_mb", vram_usage_mb)?;
engine_metrics.update_gauge("engine.memory.vram_peak_mb", vram_peak_mb)?;
```

### Registry-Based Access

All monitors are accessible through the centralized registry:

```rust
// Get memory monitor from registry
if let Some(memory_monitor) = get_registered_monitor::<MemoryResourceMonitor>("SystemRAM") {
    if let Some(report) = memory_monitor.get_memory_report() {
        println!("Current memory: {} KB", report.current_usage_bytes / 1024);
        println!("Peak memory: {} KB", report.peak_usage_bytes / 1024);
    }
}
```
        let heap_mb = memory_stats.total_allocated_bytes as f32 / 1_048_576.0;
        
        if heap_mb > self.heap_warning_threshold_mb && 
           self.last_warning_time.elapsed() > self.warning_cooldown {
            self.last_warning_time = Instant::now();
            return Some(MemoryPressureEvent::HeapPressure(heap_mb));
        }
        
        None
    }
}
```

## Metrics Collection

### Performance Metrics System

Centralized collection of all performance metrics:

```rust
pub struct PerformanceMetrics {
    cpu_metrics: CpuMetrics,
    gpu_metrics: GpuMetrics,
    memory_metrics: MemoryMetrics,
    frame_history: VecDeque<FrameMetrics>,
    collection_interval: Duration,
    last_collection: Instant,
}

#[derive(Debug, Clone)]
pub struct FrameMetrics {
    pub frame_number: u64,
    pub timestamp: Instant,
    pub total_time_ms: f32,
    pub cpu_time_ms: f32,
    pub gpu_time_ms: f32,
    pub memory_mb: f32,
    pub vram_mb: f32,
}
```

### Metrics Aggregation

The system can compute aggregate statistics:

```rust
impl PerformanceMetrics {
    pub fn get_summary(&self, duration: Duration) -> MetricsSummary {
        let cutoff = Instant::now() - duration;
        let recent_frames: Vec<_> = self.frame_history
            .iter()
            .filter(|frame| frame.timestamp > cutoff)
            .collect();
        
        if recent_frames.is_empty() {
            return MetricsSummary::default();
        }
        
        let total_frames = recent_frames.len() as f32;
        let avg_frame_time = recent_frames.iter()
            .map(|f| f.total_time_ms)
            .sum::<f32>() / total_frames;
        
        let max_frame_time = recent_frames.iter()
            .map(|f| f.total_time_ms)
            .fold(0.0f32, f32::max);
        
        let min_frame_time = recent_frames.iter()
            .map(|f| f.total_time_ms)
            .fold(f32::INFINITY, f32::min);
        
        MetricsSummary {
            duration,
            frame_count: recent_frames.len(),
            avg_frame_time_ms: avg_frame_time,
            max_frame_time_ms: max_frame_time,
            min_frame_time_ms: min_frame_time,
            avg_fps: 1000.0 / avg_frame_time,
            // ... other statistics
        }
    }
}
```

## Performance Analysis

### Bottleneck Detection

Automatic detection of performance bottlenecks:

```rust
pub struct BottleneckDetector {
    cpu_threshold_ms: f32,
    gpu_threshold_ms: f32,
    memory_threshold_mb: f32,
}

impl BottleneckDetector {
    pub fn analyze_frame(&self, metrics: &FrameMetrics) -> Vec<PerformanceIssue> {
        let mut issues = Vec::new();
        
        if metrics.cpu_time_ms > self.cpu_threshold_ms {
            issues.push(PerformanceIssue::CpuBottleneck {
                time_ms: metrics.cpu_time_ms,
                threshold_ms: self.cpu_threshold_ms,
            });
        }
        
        if metrics.gpu_time_ms > self.gpu_threshold_ms {
            issues.push(PerformanceIssue::GpuBottleneck {
                time_ms: metrics.gpu_time_ms,
                threshold_ms: self.gpu_threshold_ms,
            });
        }
        
        if metrics.memory_mb > self.memory_threshold_mb {
            issues.push(PerformanceIssue::MemoryPressure {
                usage_mb: metrics.memory_mb,
                threshold_mb: self.memory_threshold_mb,
            });
        }
        
        issues
    }
}
```

### Performance Trends

Track performance trends over time:

```rust
pub struct PerformanceTrendAnalyzer {
    samples: VecDeque<TrendSample>,
    sample_interval: Duration,
    trend_window: Duration,
}

#[derive(Debug, Clone)]
pub struct PerformanceTrend {
    pub metric: String,
    pub direction: TrendDirection,
    pub magnitude: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub enum TrendDirection {
    Improving,
    Degrading,
    Stable,
}

impl PerformanceTrendAnalyzer {
    pub fn analyze_trends(&self) -> Vec<PerformanceTrend> {
        // Analyze frame time trends
        let frame_time_trend = self.analyze_metric_trend("frame_time_ms", |sample| sample.frame_time_ms);
        
        // Analyze memory trends
        let memory_trend = self.analyze_metric_trend("memory_mb", |sample| sample.memory_mb);
        
        vec![frame_time_trend, memory_trend]
    }
}
```

## Usage Examples

### Basic Performance Monitoring

```rust
use khora_engine_core::core::monitoring::PerformanceMetrics;

fn main_loop(mut engine: Engine, mut metrics: PerformanceMetrics) {
    loop {
        let frame_start = Instant::now();
        
        // Update engine
        engine.update()?;
        
        // Render frame
        let render_stats = engine.render()?;
        
        // Collect metrics
        let frame_time = frame_start.elapsed().as_secs_f32() * 1000.0;
        metrics.record_frame(FrameMetrics {
            frame_number: engine.frame_count(),
            timestamp: Instant::now(),
            total_time_ms: frame_time,
            cpu_time_ms: render_stats.cpu_preparation_time_ms + render_stats.cpu_render_submission_time_ms,
            gpu_time_ms: render_stats.gpu_frame_total_time_ms,
            memory_mb: get_allocation_stats().total_allocated_bytes as f32 / 1_048_576.0,
            vram_mb: render_stats.vram_used_mb,
        });
        
        // Log periodic summary
        if engine.frame_count() % 300 == 0 { // Every 5 seconds at 60fps
            let summary = metrics.get_summary(Duration::from_secs(5));
            log::info!(
                "Performance: {:.1}fps avg, {:.2}ms frame, {:.1}MB memory",
                summary.avg_fps,
                summary.avg_frame_time_ms,
                summary.avg_memory_mb
            );
        }
    }
}
```

### Performance-Based Quality Adjustment

```rust
pub struct AdaptiveQualityManager {
    target_frame_time_ms: f32,
    current_quality: u32,
    adjustment_cooldown: Duration,
    last_adjustment: Instant,
    performance_buffer: VecDeque<f32>,
}

impl AdaptiveQualityManager {
    pub fn update(&mut self, frame_stats: &FrameMetrics) {
        self.performance_buffer.push_back(frame_stats.total_time_ms);
        
        // Keep only recent samples
        if self.performance_buffer.len() > 60 { // 1 second at 60fps
            self.performance_buffer.pop_front();
        }
        
        // Check if adjustment is needed
        if self.last_adjustment.elapsed() > self.adjustment_cooldown {
            let avg_frame_time = self.performance_buffer.iter().sum::<f32>() / self.performance_buffer.len() as f32;
            
            if avg_frame_time > self.target_frame_time_ms * 1.1 && self.current_quality > 1 {
                self.current_quality -= 1;
                self.last_adjustment = Instant::now();
                log::info!("Reduced quality to {} due to performance", self.current_quality);
            } else if avg_frame_time < self.target_frame_time_ms * 0.8 && self.current_quality < 3 {
                self.current_quality += 1;
                self.last_adjustment = Instant::now();
                log::info!("Increased quality to {} due to good performance", self.current_quality);
            }
        }
    }
}
```

### Memory Usage Monitoring

```rust
pub struct MemoryMonitor {
    heap_budget_mb: f32,
    vram_budget_mb: f32,
    warning_logged: bool,
}

impl MemoryMonitor {
    pub fn check_memory_health(&mut self) -> MemoryHealth {
        let heap_stats = get_allocation_stats();
        let heap_mb = heap_stats.total_allocated_bytes as f32 / 1_048_576.0;
        
        // Check heap usage
        if heap_mb > self.heap_budget_mb {
            if !self.warning_logged {
                log::warn!("Heap memory over budget: {:.1}MB > {:.1}MB", heap_mb, self.heap_budget_mb);
                self.warning_logged = true;
            }
            return MemoryHealth::Critical;
        } else if heap_mb > self.heap_budget_mb * 0.8 {
            return MemoryHealth::Warning;
        }
        
        self.warning_logged = false;
        MemoryHealth::Good
    }
}
```

### GPU Performance Analysis

```rust
pub struct GpuPerformanceAnalyzer {
    frame_time_history: VecDeque<f32>,
    target_fps: f32,
    performance_grade: PerformanceGrade,
}

impl GpuPerformanceAnalyzer {
    pub fn analyze_gpu_performance(&mut self, gpu_stats: &GpuStats) {
        self.frame_time_history.push_back(gpu_stats.frame_time_ms);
        
        // Keep only recent history
        if self.frame_time_history.len() > 120 { // 2 seconds at 60fps
            self.frame_time_history.pop_front();
        }
        
        let avg_frame_time = self.frame_time_history.iter().sum::<f32>() / self.frame_time_history.len() as f32;
        let target_frame_time = 1000.0 / self.target_fps;
        
        self.performance_grade = if avg_frame_time < target_frame_time * 0.7 {
            PerformanceGrade::Excellent
        } else if avg_frame_time < target_frame_time {
            PerformanceGrade::Good
        } else if avg_frame_time < target_frame_time * 1.5 {
            PerformanceGrade::Fair
        } else {
            PerformanceGrade::Poor
        };
    }
}
```

This performance monitoring system provides comprehensive insights into engine performance, enabling developers to identify bottlenecks and optimize application performance effectively.

For implementation details, see the source code in `khora_engine_core/src/core/monitoring.rs` and `khora_engine_core/src/core/timer.rs`.
