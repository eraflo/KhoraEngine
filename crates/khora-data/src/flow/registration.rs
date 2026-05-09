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

//! Auto-registration of [`Flow`](super::Flow) implementations via `inventory`.

use khora_core::control::gorna::ResourceBudget;
use khora_core::lane::LaneBus;
use khora_core::Runtime;

use crate::ecs::{SemanticDomain, World};

/// Registration entry for a [`Flow`](super::Flow) — submitted by each
/// concrete Flow implementation via [`inventory::submit!`].
///
/// Type erasure: because [`Flow`](super::Flow) has an associated `View`
/// type, we cannot store trait objects directly. Each Flow provides a
/// trampoline `run` function that owns its lifecycle (typically delegating
/// to a `static OnceLock<Mutex<MyFlow>>` instance) and publishes its View
/// into the [`LaneBus`].
pub struct FlowRegistration {
    /// Stable identifier — matches `Flow::NAME`.
    pub name: &'static str,
    /// Domain this Flow serves — matches `Flow::DOMAIN`.
    pub domain: SemanticDomain,
    /// Trampoline that runs select + adapt + project and publishes the View.
    pub run: fn(&mut World, &mut LaneBus, &ResourceBudget, &Runtime),
}

inventory::collect!(FlowRegistration);

/// Convenience macro for declaring a Flow registration with the standard
/// trampoline (single static instance, no per-frame allocation).
///
/// # Example
///
/// ```rust,ignore
/// use khora_data::flow::register_flow;
///
/// pub struct MyFlow { /* ... */ }
/// impl khora_data::flow::Flow for MyFlow { /* ... */ }
///
/// register_flow!(MyFlow);
/// ```
#[macro_export]
macro_rules! register_flow {
    ($flow_ty:ty) => {
        const _: () = {
            fn run_flow(
                world: &mut $crate::ecs::World,
                bus: &mut khora_core::lane::LaneBus,
                budget: &khora_core::control::gorna::ResourceBudget,
                runtime: &khora_core::Runtime,
            ) {
                use std::sync::{Mutex, OnceLock};
                static INSTANCE: OnceLock<Mutex<$flow_ty>> = OnceLock::new();
                let mut flow = INSTANCE
                    .get_or_init(|| Mutex::new(<$flow_ty as Default>::default()))
                    .lock()
                    .expect("Flow mutex poisoned");
                let sel = <$flow_ty as $crate::flow::Flow>::select(&mut flow, world, runtime);
                <$flow_ty as $crate::flow::Flow>::adapt(&mut flow, world, &sel, budget, runtime);
                let view = <$flow_ty as $crate::flow::Flow>::project(&flow, world, &sel, runtime);
                bus.publish(view);
            }
            inventory::submit! {
                $crate::flow::FlowRegistration {
                    name: <$flow_ty as $crate::flow::Flow>::NAME,
                    domain: <$flow_ty as $crate::flow::Flow>::DOMAIN,
                    run: run_flow,
                }
            }
        };
    };
}
