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

//! What a lane is: the [`Lane`] trait, its family, levels and costs, its typed
//! inputs and outputs, and the checks that hold it to them.

mod device;
mod error;
mod input_warning;
mod lane;
mod output_ops;
mod output_refusal;
mod validate;

pub mod compile_check;
pub mod cost;
pub mod erased;
pub mod family;
pub mod guard;
pub mod io;
pub mod level;

pub use error::LaneError;
pub use lane::{Lane, LaneReport};
