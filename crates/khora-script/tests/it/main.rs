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

//! khora-script integration tests, compiled as one test binary.
//!
//! Each file under `tests/` is its own executable, and each executable is a
//! full link of the crate's dependency tree. New integration tests join this
//! binary as a module instead of adding a file at the root of `tests/`.

mod array_checks;
mod array_element_types;
mod array_fuel;
mod array_growth;
mod array_growth_checks;
mod array_growth_fuel;
mod array_paths;
mod arrays;
mod await_takes_a_duration;
mod behavior_component_writes;
mod behavior_components;
mod behavior_default_values;
mod behavior_defaults;
mod bound_name_shadowing;
mod calls_through_an_entity;
mod coalesce_and_casts;
mod component_reads;
mod component_write_receivers;
mod component_writes;
mod component_writes_under_fuel;
mod compound_field_writes_under_fuel;
mod conditional_array_types;
mod conformance;
mod conformance_edges;
mod control_flow;
mod control_flow_checks;
mod control_flow_resume;
mod declared_components;
mod durable_text;
mod engine_arithmetic;
mod entity_field_reads;
mod entity_methods;
mod ergon_fn;
mod events_declared;
mod events_delivered;
mod events_handled;
mod events_raised;
mod field_path_writes_under_fuel;
mod fingerprints;
mod fixed_update_declared;
mod freeze;
mod held_array_growth;
mod held_arrays;
mod held_structs;
mod held_text;
mod match_arm_edits;
mod match_on_a_computed_subject;
mod narrowed_frames;
mod narrowing_site_names;
mod operand_types;
mod optional_array_methods;
mod optional_array_receivers;
mod overdraft_bound;
mod public_paths;
mod queries_across_an_edit;
mod queries_in_behaviors;
mod queries_under_fuel;
mod query_binding_types;
mod query_rows;
mod query_sites;
mod rebuilt_frames;
mod rebuilt_local_types;
mod rebuilt_machines_holding_edited_structs;
mod ref_write_rows;
mod refused_by_the_checker;
mod removals_in_a_loop_under_fuel;
mod resume_tiers;
mod safepoints;
mod saved_values_into_declared_structs;
mod scheduling_attributes;
mod script_component_declarations;
mod self_referential_struct_defaults;
mod sites;
mod sized_operation_sites;
mod source_positions;
mod source_positions_across_the_import_graph;
mod state_call_resolution;
mod state_entry_dispatch;
mod state_scope;
mod struct_checks;
mod struct_default_cycles;
mod struct_edits_across_a_resume;
mod struct_field_defaults;
mod struct_literal_edits_under_fuel;
mod structs;
mod structs_in_fields;
mod support;
mod system_component_reach;
mod system_components;
mod system_subject_reads;
mod systems_front_end;
mod unchanged_frames_holding_edited_structs;
mod unit_field_types;
mod zero_values;
