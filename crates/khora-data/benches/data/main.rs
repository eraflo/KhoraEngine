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

//! The data layer's benchmarks, in one binary: queries, scene loads, and the
//! script commands' writes. One binary links the crate and criterion once,
//! where one per benchmark linked them each time.

mod query;
mod scene_load;
mod script_commands;

criterion::criterion_main!(
    query::benches,
    scene_load::benches,
    script_commands::benches
);
