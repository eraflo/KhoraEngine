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

//! Compile-level guard over `khora_control`'s public surface.
//!
//! Every `pub` item reachable through a `pub` path of the crate is named here at
//! its full path: the crate-root re-exports, each `pub mod`, their types, free
//! functions, constants, public fields, enum variants, inherent `pub` methods and
//! derived trait impls. A reorganisation that moves code between files must keep
//! every one of these paths valid, so this file stops compiling the moment one
//! disappears. Re-exports are also checked for *identity*: a `pub use` that
//! starts pointing at a different item with the same name is a compile error.
//!
//! Nothing is constructed; the single test only has to type-check.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_eq<T: Eq>() {}
fn is_partial_eq<T: PartialEq>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_control::agent_registry as _;
    use khora_control::analysis as _;
    use khora_control::budget_channel as _;
    use khora_control::cost_model as _;
    use khora_control::dcc_context as _;
    use khora_control::dcc_service as _;
    use khora_control::gorna as _;
    use khora_control::metrics as _;
    use khora_control::pid as _;
    use khora_control::plugin as _;
    use khora_control::scheduler as _;
    use khora_control::substrate as _;
    use khora_control::substrate::flow_runner as _;
    use khora_control::worker_pool as _;
}

// ---------------------------------------------------------------------------
// Public fields — read through a reference, never constructed.
// ---------------------------------------------------------------------------

fn analysis_report_fields(r: &khora_control::analysis::AnalysisReport) {
    let _ = (
        &r.needs_negotiation,
        &r.suggested_latency_ms,
        &r.death_spiral_detected,
        &r.alerts,
    );
}

fn hardware_state_fields(h: &khora_control::dcc_context::HardwareState) {
    let _ = (
        &h.thermal,
        &h.battery,
        &h.cpu_load,
        &h.gpu_load,
        &h.available_vram,
        &h.total_vram,
        &h.current_ram_bytes,
        &h.memory_budget_bytes,
    );
}

fn context_fields(c: &khora_control::dcc_context::Context) {
    let _ = (
        &c.hardware,
        &c.mode,
        &c.global_budget_multiplier,
        &c.memory_pressure,
    );
}

fn cost_sample_fields(s: &khora_control::cost_model::CostSample) {
    let _ = (&s.n, &s.time_ms);
}

fn dcc_config_fields(c: &khora_control::dcc_service::DccConfig) {
    let _ = (
        &c.tick_rate,
        &c.telemetry_buffer_size,
        &c.agent_lock_timeout_ms,
        &c.memory_budget_bytes,
        &c.frame_pid,
    );
}

fn pid_config_fields(x: &khora_control::pid::PidConfig) {
    let _ = (
        &x.kp,
        &x.ki,
        &x.kd,
        &x.setpoint_weight_b,
        &x.derivative_filter_n,
        &x.kb,
        &x.output_min,
        &x.output_max,
    );
}

/// The DCC's frame PID is tuned with this crate's `PidConfig`.
fn dcc_config_frame_pid(
    c: &khora_control::dcc_service::DccConfig,
) -> &khora_control::pid::PidConfig {
    &c.frame_pid
}

/// `EnginePlugin::on_phase` takes an `impl Fn`, so it cannot be named as a
/// bare path; calling it from a never-run function checks it instead.
fn engine_plugin_on_phase(
    p: &mut khora_control::plugin::EnginePlugin,
    phase: khora_core::agent::ExecutionPhase,
) {
    p.on_phase(phase, |_: &mut khora_data::ecs::World| {});
}

// ---------------------------------------------------------------------------
// Public constants.
// ---------------------------------------------------------------------------

// module `dcc_context`
const _: f32 = khora_control::dcc_context::MEMORY_PRESSURE_CRITICAL;
// module `cost_model`
const _: [khora_control::cost_model::ComplexityClass; 4] =
    khora_control::cost_model::ComplexityClass::ALL;

#[test]
fn every_public_path_of_khora_control_still_resolves() {
    // --- crate root: `pub use` re-exports, and that each points at its item.
    let _ = type_name::<khora_control::AnalysisReport>();
    let _ = type_name::<khora_control::BatteryLevel>();
    let _ = type_name::<khora_control::Context>();
    let _ = type_name::<khora_control::EngineMode>();
    let _ = type_name::<khora_control::HardwareState>();
    let _ = type_name::<khora_control::ThermalStatus>();
    let _ = type_name::<khora_control::GornaArbitrator>();
    let _ = type_name::<khora_control::EnginePlugin>();
    let _ = type_name::<khora_control::AgentRegistry>();
    let _ = type_name::<khora_control::ExecutionScheduler>();
    let _ = type_name::<khora_control::DccConfig>();
    let _ = type_name::<khora_control::DccService>();
    same_type(
        PhantomData::<khora_control::AnalysisReport>,
        PhantomData::<khora_control::analysis::AnalysisReport>,
    );
    same_type(
        PhantomData::<khora_control::BatteryLevel>,
        PhantomData::<khora_control::dcc_context::BatteryLevel>,
    );
    same_type(
        PhantomData::<khora_control::Context>,
        PhantomData::<khora_control::dcc_context::Context>,
    );
    same_type(
        PhantomData::<khora_control::EngineMode>,
        PhantomData::<khora_control::dcc_context::EngineMode>,
    );
    same_type(
        PhantomData::<khora_control::HardwareState>,
        PhantomData::<khora_control::dcc_context::HardwareState>,
    );
    same_type(
        PhantomData::<khora_control::ThermalStatus>,
        PhantomData::<khora_control::dcc_context::ThermalStatus>,
    );
    same_type(
        PhantomData::<khora_control::GornaArbitrator>,
        PhantomData::<khora_control::gorna::GornaArbitrator>,
    );
    same_type(
        PhantomData::<khora_control::EnginePlugin>,
        PhantomData::<khora_control::plugin::EnginePlugin>,
    );
    same_type(
        PhantomData::<khora_control::AgentRegistry>,
        PhantomData::<khora_control::agent_registry::AgentRegistry>,
    );
    same_type(
        PhantomData::<khora_control::ExecutionScheduler>,
        PhantomData::<khora_control::scheduler::ExecutionScheduler>,
    );
    same_type(
        PhantomData::<khora_control::DccConfig>,
        PhantomData::<khora_control::dcc_service::DccConfig>,
    );
    same_type(
        PhantomData::<khora_control::DccService>,
        PhantomData::<khora_control::dcc_service::DccService>,
    );

    // --- module `analysis`
    let _ = type_name::<khora_control::analysis::AnalysisReport>();
    let _ = type_name::<khora_control::analysis::HeuristicEngine>();
    let _ = analysis_report_fields as fn(&khora_control::analysis::AnalysisReport);
    is_default::<khora_control::analysis::AnalysisReport>();
    is_clone::<khora_control::analysis::AnalysisReport>();
    is_debug::<khora_control::analysis::AnalysisReport>();
    let _ = khora_control::analysis::HeuristicEngine::analyze;

    // --- module `budget_channel`
    let _ = type_name::<khora_control::budget_channel::BudgetChannel>();
    is_clone::<khora_control::budget_channel::BudgetChannel>();
    let _ = khora_control::budget_channel::BudgetChannel::new;
    let _ = khora_control::budget_channel::BudgetChannel::send;
    let _ = khora_control::budget_channel::BudgetChannel::sync;
    let _ = khora_control::budget_channel::BudgetChannel::get;

    // --- module `dcc_context` (re-exports of `khora_core` enums + own items)
    let _ = type_name::<khora_control::dcc_context::EngineMode>();
    let _ = type_name::<khora_control::dcc_context::BatteryLevel>();
    let _ = type_name::<khora_control::dcc_context::ThermalStatus>();
    let _ = type_name::<khora_control::dcc_context::HardwareState>();
    let _ = type_name::<khora_control::dcc_context::Context>();
    same_type(
        PhantomData::<khora_control::dcc_context::EngineMode>,
        PhantomData::<khora_core::agent::EngineMode>,
    );
    same_type(
        PhantomData::<khora_control::dcc_context::BatteryLevel>,
        PhantomData::<khora_core::platform::BatteryLevel>,
    );
    same_type(
        PhantomData::<khora_control::dcc_context::ThermalStatus>,
        PhantomData::<khora_core::platform::ThermalStatus>,
    );
    let _ = hardware_state_fields as fn(&khora_control::dcc_context::HardwareState);
    let _ = context_fields as fn(&khora_control::dcc_context::Context);
    is_default::<khora_control::dcc_context::HardwareState>();
    is_clone::<khora_control::dcc_context::HardwareState>();
    is_debug::<khora_control::dcc_context::HardwareState>();
    is_default::<khora_control::dcc_context::Context>();
    is_clone::<khora_control::dcc_context::Context>();
    is_debug::<khora_control::dcc_context::Context>();
    let _ = khora_control::dcc_context::Context::refresh_memory_pressure;
    let _ = khora_control::dcc_context::MEMORY_PRESSURE_CRITICAL;
    let _ = khora_control::dcc_context::safety_ceiling;

    // --- module `cost_model`
    let _ = type_name::<khora_control::cost_model::ComplexityClass>();
    let _ = type_name::<khora_control::cost_model::CostSample>();
    let _ = type_name::<khora_control::cost_model::CostModel>();
    let _ = [
        khora_control::cost_model::ComplexityClass::Constant,
        khora_control::cost_model::ComplexityClass::Linear,
        khora_control::cost_model::ComplexityClass::Linearithmic,
        khora_control::cost_model::ComplexityClass::Quadratic,
    ];
    let _ = khora_control::cost_model::ComplexityClass::ALL;
    let _ = khora_control::cost_model::ComplexityClass::f;
    is_copy::<khora_control::cost_model::ComplexityClass>();
    is_eq::<khora_control::cost_model::ComplexityClass>();
    is_debug::<khora_control::cost_model::ComplexityClass>();
    let _ = cost_sample_fields as fn(&khora_control::cost_model::CostSample);
    is_copy::<khora_control::cost_model::CostSample>();
    is_debug::<khora_control::cost_model::CostSample>();
    is_clone::<khora_control::cost_model::CostModel>();
    is_debug::<khora_control::cost_model::CostModel>();
    let _ = khora_control::cost_model::CostModel::new;
    let _ = khora_control::cost_model::CostModel::record;
    let _ = khora_control::cost_model::CostModel::len;
    let _ = khora_control::cost_model::CostModel::is_empty;
    let _ = khora_control::cost_model::CostModel::best_fit;
    let _ = khora_control::cost_model::CostModel::predict_ms;
    let _ = khora_control::cost_model::CostModel::latest_ms;

    // --- module `gorna`
    let _ = type_name::<khora_control::gorna::GornaArbitrator>();
    let _ = khora_control::gorna::GornaArbitrator::new;
    let _ = khora_control::gorna::GornaArbitrator::set_wave_plan;
    let _ = khora_control::gorna::GornaArbitrator::set_adaptation_mode;
    let _ = khora_control::gorna::GornaArbitrator::adaptation_mode;
    let _ = khora_control::gorna::GornaArbitrator::arbitrate;

    // --- module `metrics`
    let _ = type_name::<khora_control::metrics::RingBuffer<f32, 4>>();
    let _ = type_name::<khora_control::metrics::MetricStore>();
    is_default::<khora_control::metrics::RingBuffer<f32, 4>>();
    is_clone::<khora_control::metrics::RingBuffer<f32, 4>>();
    is_debug::<khora_control::metrics::RingBuffer<f32, 4>>();
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::new;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::push;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::count;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::iter;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::average;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::trend;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::variance;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::min;
    let _ = khora_control::metrics::RingBuffer::<f32, 4>::max;
    is_default::<khora_control::metrics::MetricStore>();
    is_debug::<khora_control::metrics::MetricStore>();
    let _ = khora_control::metrics::MetricStore::new;
    let _ = khora_control::metrics::MetricStore::push;
    let _ = khora_control::metrics::MetricStore::get_average;
    let _ = khora_control::metrics::MetricStore::get_trend;
    let _ = khora_control::metrics::MetricStore::get_variance;
    let _ = khora_control::metrics::MetricStore::get_max;
    let _ = khora_control::metrics::MetricStore::get_min;
    let _ = khora_control::metrics::MetricStore::get_sample_count;

    // --- module `pid`
    let _ = type_name::<khora_control::pid::PidConfig>();
    let _ = pid_config_fields as fn(&khora_control::pid::PidConfig);
    is_debug::<khora_control::pid::PidConfig>();
    is_clone::<khora_control::pid::PidConfig>();
    is_copy::<khora_control::pid::PidConfig>();
    is_partial_eq::<khora_control::pid::PidConfig>();
    is_default::<khora_control::pid::PidConfig>();
    let _ = type_name::<khora_control::pid::PidController>();
    let _ = khora_control::pid::PidController::new;
    let _ = khora_control::pid::PidController::update;
    let _ = khora_control::pid::PidController::reset;
    let _ = khora_control::pid::PidController::output;
    let _ = khora_control::pid::PidController::config;
    is_debug::<khora_control::pid::PidController>();
    is_clone::<khora_control::pid::PidController>();

    // --- module `plugin`
    let _ = type_name::<khora_control::plugin::EnginePlugin>();
    let _ = khora_control::plugin::EnginePlugin::new;
    let _ = engine_plugin_on_phase
        as fn(&mut khora_control::plugin::EnginePlugin, khora_core::agent::ExecutionPhase);
    let _ = khora_control::plugin::EnginePlugin::execute;
    let _ = khora_control::plugin::EnginePlugin::wants_phase;
    let _ = khora_control::plugin::EnginePlugin::name;

    // --- module `agent_registry`
    let _ = type_name::<khora_control::agent_registry::PhaseAgentEntry>();
    let _ = type_name::<khora_control::agent_registry::AgentRegistry>();
    is_default::<khora_control::agent_registry::AgentRegistry>();
    let _ = khora_control::agent_registry::AgentRegistry::new;
    let _ = khora_control::agent_registry::AgentRegistry::register;
    let _ = khora_control::agent_registry::AgentRegistry::register_for_mode;
    let _ = khora_control::agent_registry::AgentRegistry::len;
    let _ = khora_control::agent_registry::AgentRegistry::is_empty;
    let _ = khora_control::agent_registry::AgentRegistry::iter;
    let _ = khora_control::agent_registry::AgentRegistry::all_ids;
    let _ = khora_control::agent_registry::AgentRegistry::initialize_all;
    let _ = khora_control::agent_registry::AgentRegistry::execute_all;
    let _ = khora_control::agent_registry::AgentRegistry::get_by_id;
    let _ = khora_control::agent_registry::AgentRegistry::collect_for_phase;
    let _ = khora_control::agent_registry::AgentRegistry::execute_agent;

    // --- module `scheduler`
    let _ = type_name::<khora_control::scheduler::ExecutionScheduler>();
    let _ = khora_control::scheduler::ExecutionScheduler::new;
    let _ = khora_control::scheduler::ExecutionScheduler::set_parallel_execution;
    let _ = khora_control::scheduler::ExecutionScheduler::set_telemetry_sender;
    let _ = khora_control::scheduler::ExecutionScheduler::deck_mut;
    let _ = khora_control::scheduler::ExecutionScheduler::budget_channel;
    let _ = khora_control::scheduler::ExecutionScheduler::register_plugin;
    let _ = khora_control::scheduler::ExecutionScheduler::set_phase_order;
    let _ = khora_control::scheduler::ExecutionScheduler::insert_after;
    let _ = khora_control::scheduler::ExecutionScheduler::insert_before;
    let _ = khora_control::scheduler::ExecutionScheduler::remove_phase;
    let _ = khora_control::scheduler::ExecutionScheduler::run_frame;

    // --- module `dcc_service`
    let _ = type_name::<khora_control::dcc_service::DccConfig>();
    let _ = type_name::<khora_control::dcc_service::DccService>();
    let _ = dcc_config_fields as fn(&khora_control::dcc_service::DccConfig);
    let _ = dcc_config_frame_pid
        as fn(&khora_control::dcc_service::DccConfig) -> &khora_control::pid::PidConfig;
    is_default::<khora_control::dcc_service::DccConfig>();
    is_clone::<khora_control::dcc_service::DccConfig>();
    is_debug::<khora_control::dcc_service::DccConfig>();
    let _ = khora_control::dcc_service::DccService::new;
    let _ = khora_control::dcc_service::DccService::set_adaptation_mode;
    let _ = khora_control::dcc_service::DccService::adaptation_mode;
    let _ = khora_control::dcc_service::DccService::set_hint;
    let _ = khora_control::dcc_service::DccService::clear_agent_hints;
    let _ = khora_control::dcc_service::DccService::hints;
    let _ = khora_control::dcc_service::DccService::layout_recommendations;
    let _ = khora_control::dcc_service::DccService::start_decision_recording;
    let _ = khora_control::dcc_service::DccService::stop_decision_recording;
    let _ = khora_control::dcc_service::DccService::recorded_decisions;
    let _ = khora_control::dcc_service::DccService::replay_decisions;
    let _ = khora_control::dcc_service::DccService::stop_replay;
    let _ = khora_control::dcc_service::DccService::is_replaying;
    let _ = khora_control::dcc_service::DccService::connect_budget_channel;
    let _ = khora_control::dcc_service::DccService::register_agent;
    let _ = khora_control::dcc_service::DccService::register_agent_for_mode;
    let _ = khora_control::dcc_service::DccService::start;
    let _ = khora_control::dcc_service::DccService::stop;
    let _ = khora_control::dcc_service::DccService::agent_registry;
    let _ = khora_control::dcc_service::DccService::event_sender;
    let _ = khora_control::dcc_service::DccService::get_context;
    let _ = khora_control::dcc_service::DccService::context_handle;
    let _ = khora_control::dcc_service::DccService::initialize_agents;
    let _ = khora_control::dcc_service::DccService::execute_agents;
    let _ = khora_control::dcc_service::DccService::agent_count;
    let _ = khora_control::dcc_service::DccService::get_agent;

    // --- module `substrate` (used by khora-sdk and khora-agents tests)
    let _ = khora_control::substrate::run_data_systems;
    let _ = khora_control::substrate::run_flows;
    // --- module `substrate::flow_runner`
    let _ = khora_control::substrate::flow_runner::run_flows;
    same_item(
        &khora_control::substrate::run_flows,
        &khora_control::substrate::flow_runner::run_flows,
    );

    // --- module `worker_pool`
    let _ = type_name::<khora_control::worker_pool::WorkerPool>();
    let _ = khora_control::worker_pool::WorkerPool::new;
    let _ = khora_control::worker_pool::WorkerPool::thread_count;
    let _ = khora_control::worker_pool::WorkerPool::submit::<fn()>;
}

// ---------------------------------------------------------------------------
// Paths other crates import today, spelled exactly as they spell them.
// ---------------------------------------------------------------------------

// khora-sdk/src/engine/ (and khora-sdk/src/lib.rs for the last three)
use khora_control::{substrate, DccConfig, DccService, EngineMode};
// khora-sdk/src/lib.rs
use khora_control::agent_registry::AgentRegistry;
use khora_control::Context as DccContext;
use khora_control::Context as EngineContext;

#[test]
fn paths_used_by_other_crates_still_resolve() {
    // khora-sdk/src/engine/: `substrate::run_data_systems`, `ExecutionScheduler`,
    // `Context { hardware: HardwareState::default(), .. }`.
    let _ = substrate::run_data_systems;
    let _ = type_name::<khora_control::ExecutionScheduler>();
    let _ = type_name::<khora_control::Context>();
    let _ = type_name::<khora_control::HardwareState>();
    let _ = type_name::<DccConfig>();
    let _ = type_name::<DccService>();
    let _ = type_name::<EngineMode>();
    let _ = khora_control::ExecutionScheduler::new;
    // khora-sdk/src/lib.rs
    let _ = type_name::<AgentRegistry>();
    same_type(PhantomData::<DccContext>, PhantomData::<EngineContext>);
    // khora-agents/tests/{frame_e2e_test,input_from_script_test,physics_agent_tests}.rs
    let _ = khora_control::substrate::run_flows;
    let _ = khora_control::substrate::run_data_systems;
    let _ = substrate::run_flows;
}
