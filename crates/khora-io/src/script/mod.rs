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

//! Ergon scripts on the I/O side: reading modules from disk ([`compile`]),
//! mirroring components into Ergon declarations ([`mirror`]), and reloading a
//! changed file ([`hot_reload`]).

pub mod compile;
pub mod hot_reload;
pub mod mirror;
