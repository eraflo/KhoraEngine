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

//! A value's own check, opted into by a lane's typed inputs or outputs.

use super::guard::ValueProblem;

/// A value's own check, opted into with `#[bus(validate)]` or
/// `#[deck(validate)]`.
///
/// It names what is wrong and never rewrites it: a problem warns, the lane
/// still runs on the value as it is.
#[diagnostic::on_unimplemented(
    message = "`validate` checks `{Self}` with its `Validate` impl, and `{Self}` has none",
    label = "no `Validate` impl"
)]
pub trait Validate {
    /// Every problem in `self`; empty when nothing is wrong.
    fn validate(&self) -> Vec<ValueProblem>;
}
