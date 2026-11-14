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

//! Ergon scripts on the I/O side: reading, mirroring, compiling and reloading
//! a project's modules.

pub mod compile;
pub mod compile_hub;
pub mod declarations;
pub mod hot_reload;
pub mod layout;
pub mod mirror;
pub mod mounted;
pub mod problems;
pub mod project_compiler;
