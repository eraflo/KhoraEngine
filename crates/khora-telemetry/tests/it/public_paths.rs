// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Compile-level guard over `khora_telemetry`'s public surface, plus the paths
//! other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), and the
//! trait impls. A reorganisation that moves code between files must keep every
//! one of these paths valid, so this module stops compiling the moment one
//! disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions).
//!
//! The list was generated from `khora_telemetry`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.
//!
//! `khora-io` reaches into the submodules (`metrics::{CounterHandle,
//! HistogramHandle}`), so they are pinned at those paths as well as at the
//! crate-root re-exports.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

fn is_clone<T: Clone>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_metrics_backend<T: khora_telemetry::storage::backend::MetricsBackend>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_telemetry::metrics as _;
    use khora_telemetry::monitoring as _;
    use khora_telemetry::scoped_metric_timer as _;
    use khora_telemetry::storage as _;
    use khora_telemetry::storage::backend as _;
    use khora_telemetry::storage::memory_backend as _;
    use khora_telemetry::telemetry_service as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn backend_stats_fields(x: &khora_telemetry::storage::backend::BackendStats) {
    let _ = (
        &x.total_metrics,
        &x.counter_count,
        &x.gauge_count,
        &x.histogram_count,
        &x.estimated_memory_bytes,
    );
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    fn metrics_backend_trait_items<T: khora_telemetry::storage::backend::MetricsBackend>() {
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::as_any;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::put_metric;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::get_metric;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::contains_metric;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::remove_metric;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::list_metric_ids;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::list_all_metrics;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::clear_all;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::metric_count;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::increment_counter;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::set_gauge;
        let _ = <T as khora_telemetry::storage::backend::MetricsBackend>::record_histogram_sample;
    }
}

#[test]
fn module_metrics_paths_still_resolve() {
    let _ = type_name::<khora_telemetry::metrics::CounterHandle>();
    let _ = khora_telemetry::metrics::CounterHandle::increment;
    let _ = khora_telemetry::metrics::CounterHandle::increment_by;
    let _ = khora_telemetry::metrics::CounterHandle::get;
    let _ = khora_telemetry::metrics::CounterHandle::id;
    is_debug::<khora_telemetry::metrics::CounterHandle>();
    is_clone::<khora_telemetry::metrics::CounterHandle>();
    let _ = type_name::<khora_telemetry::metrics::GaugeHandle>();
    let _ = khora_telemetry::metrics::GaugeHandle::set;
    let _ = khora_telemetry::metrics::GaugeHandle::add;
    let _ = khora_telemetry::metrics::GaugeHandle::sub;
    let _ = khora_telemetry::metrics::GaugeHandle::get;
    let _ = khora_telemetry::metrics::GaugeHandle::id;
    is_debug::<khora_telemetry::metrics::GaugeHandle>();
    is_clone::<khora_telemetry::metrics::GaugeHandle>();
    let _ = type_name::<khora_telemetry::metrics::HistogramHandle>();
    let _ = khora_telemetry::metrics::HistogramHandle::observe;
    let _ = khora_telemetry::metrics::HistogramHandle::id;
    let _ = khora_telemetry::metrics::HistogramHandle::get_metric;
    is_debug::<khora_telemetry::metrics::HistogramHandle>();
    is_clone::<khora_telemetry::metrics::HistogramHandle>();
    let _ = type_name::<khora_telemetry::metrics::MetricsRegistry>();
    let _ = type_name::<khora_telemetry::MetricsRegistry>();
    same_type(
        PhantomData::<khora_telemetry::MetricsRegistry>,
        PhantomData::<khora_telemetry::metrics::MetricsRegistry>,
    );
    let _ = khora_telemetry::metrics::MetricsRegistry::new;
    let _ = khora_telemetry::metrics::MetricsRegistry::with_backend;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_telemetry::metrics::MetricsRegistry,
        String,
        String,
        String,
    )
        -> khora_core::telemetry::MetricsResult<khora_telemetry::metrics::CounterHandle> =
        khora_telemetry::metrics::MetricsRegistry::register_counter;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_telemetry::metrics::MetricsRegistry,
        String,
        String,
        String,
        Vec<(String, String)>,
    )
        -> khora_core::telemetry::MetricsResult<khora_telemetry::metrics::CounterHandle> =
        khora_telemetry::metrics::MetricsRegistry::register_counter_with_labels;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_telemetry::metrics::MetricsRegistry,
        String,
        String,
        String,
        String,
    )
        -> khora_core::telemetry::MetricsResult<khora_telemetry::metrics::GaugeHandle> =
        khora_telemetry::metrics::MetricsRegistry::register_gauge;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_telemetry::metrics::MetricsRegistry,
        String,
        String,
        String,
        String,
        Vec<(String, String)>,
    )
        -> khora_core::telemetry::MetricsResult<khora_telemetry::metrics::GaugeHandle> =
        khora_telemetry::metrics::MetricsRegistry::register_gauge_with_labels;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_telemetry::metrics::MetricsRegistry,
        String,
        String,
        String,
        String,
        Vec<f64>,
    ) -> khora_core::telemetry::MetricsResult<
        khora_telemetry::metrics::HistogramHandle,
    > = khora_telemetry::metrics::MetricsRegistry::register_histogram;
    let _ = khora_telemetry::metrics::MetricsRegistry::get_metric;
    let _ = khora_telemetry::metrics::MetricsRegistry::contains_metric;
    let _ = khora_telemetry::metrics::MetricsRegistry::get_namespace_metrics;
    let _ = khora_telemetry::metrics::MetricsRegistry::get_all_counters;
    let _ = khora_telemetry::metrics::MetricsRegistry::get_all_gauges;
    let _ = khora_telemetry::metrics::MetricsRegistry::metric_count;
    let _ = khora_telemetry::metrics::MetricsRegistry::clear_all;
    let _ = khora_telemetry::metrics::MetricsRegistry::backend;
    is_debug::<khora_telemetry::metrics::MetricsRegistry>();
    is_default::<khora_telemetry::metrics::MetricsRegistry>();
}

#[test]
fn module_monitoring_paths_still_resolve() {
    let _ = type_name::<khora_telemetry::monitoring::MonitorRegistry>();
    let _ = type_name::<khora_telemetry::MonitorRegistry>();
    same_type(
        PhantomData::<khora_telemetry::MonitorRegistry>,
        PhantomData::<khora_telemetry::monitoring::MonitorRegistry>,
    );
    let _ = khora_telemetry::monitoring::MonitorRegistry::new;
    let _ = khora_telemetry::monitoring::MonitorRegistry::register;
    let _ = khora_telemetry::monitoring::MonitorRegistry::update_all;
    let _ = khora_telemetry::monitoring::MonitorRegistry::get_all_monitors;
    is_debug::<khora_telemetry::monitoring::MonitorRegistry>();
    is_clone::<khora_telemetry::monitoring::MonitorRegistry>();
    is_default::<khora_telemetry::monitoring::MonitorRegistry>();
}

#[test]
fn module_telemetry_service_paths_still_resolve() {
    let _ = type_name::<khora_telemetry::telemetry_service::TelemetryService>();
    let _ = type_name::<khora_telemetry::TelemetryService>();
    same_type(
        PhantomData::<khora_telemetry::TelemetryService>,
        PhantomData::<khora_telemetry::telemetry_service::TelemetryService>,
    );
    let _ = khora_telemetry::telemetry_service::TelemetryService::new;
    let _ = khora_telemetry::telemetry_service::TelemetryService::with_dcc_sender;
    let _ = khora_telemetry::telemetry_service::TelemetryService::tick;
    let _ = khora_telemetry::telemetry_service::TelemetryService::metrics_registry;
    let _ = khora_telemetry::telemetry_service::TelemetryService::monitor_registry;
    is_debug::<khora_telemetry::telemetry_service::TelemetryService>();
    is_default::<khora_telemetry::telemetry_service::TelemetryService>();
}

#[test]
fn module_storage_paths_still_resolve() {
    let _ = type_name::<khora_telemetry::storage::backend::BackendStats>();
    let _ = backend_stats_fields as fn(&khora_telemetry::storage::backend::BackendStats);
    is_debug::<khora_telemetry::storage::backend::BackendStats>();
    is_clone::<khora_telemetry::storage::backend::BackendStats>();
    // trait `khora_telemetry::storage::backend::MetricsBackend`: see `metrics_backend_trait_items`
    let _ = type_name::<khora_telemetry::storage::memory_backend::InMemoryBackend>();
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::new;
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::with_capacity;
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::get_stats;
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::get_metrics_by_namespace;
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::get_metrics_by_type;
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::put_metrics;
    let _ = khora_telemetry::storage::memory_backend::InMemoryBackend::remove_metrics_by_namespace;
    is_debug::<khora_telemetry::storage::memory_backend::InMemoryBackend>();
    is_default::<khora_telemetry::storage::memory_backend::InMemoryBackend>();
    is_metrics_backend::<khora_telemetry::storage::memory_backend::InMemoryBackend>();
}

#[test]
fn module_scoped_metric_timer_paths_still_resolve() {
    let _ = type_name::<khora_telemetry::scoped_metric_timer::ScopedMetricTimer<'static>>();
    let _ = type_name::<khora_telemetry::ScopedMetricTimer<'static>>();
    same_type(
        PhantomData::<khora_telemetry::ScopedMetricTimer<'static>>,
        PhantomData::<khora_telemetry::scoped_metric_timer::ScopedMetricTimer<'static>>,
    );
    let _ = khora_telemetry::scoped_metric_timer::ScopedMetricTimer::new;
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded, macro bodies included).
// The trailing comment names the users. Items, modules and enum variants
// are imported; associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_telemetry::metrics::CounterHandle as _; // khora-io
    use khora_telemetry::metrics::HistogramHandle as _; // khora-io
    use khora_telemetry::MetricsRegistry as _; // khora-agents, khora-io, khora-sdk
    use khora_telemetry::MonitorRegistry as _; // khora-sdk
    use khora_telemetry::ScopedMetricTimer as _; // khora-io
    use khora_telemetry::TelemetryService as _; // khora-sdk
}
