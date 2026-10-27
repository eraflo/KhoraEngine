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

//! The typed lane contract: what a lane's derived inputs declare and fetch,
//! the guards a fetch runs, the boot checks over every agent registration, and
//! the compile errors the derives and `register_agent!` give.

mod boot;
mod compile_fail;
mod fixed_step;
mod fixtures;
mod guards;
mod inputs;
mod outputs;
mod pass_device;
mod passes;
mod phases;
mod run;
