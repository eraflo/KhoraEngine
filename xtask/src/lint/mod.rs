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

//! The checks that read one file at a time: in-file order, sizes and
//! comments of a Rust file, the rules of a book figure, each also run over
//! the whole tree by `xtask/tests/layout.rs`.

pub mod allowlist;
pub mod boundaries;
pub mod code;
pub mod comments;
pub mod figures;
pub mod finding;
pub mod functions;
pub mod history;
pub mod items;
pub mod modules;
pub mod order;
pub mod primary;
pub mod runner;
pub mod scope;
pub mod summaries;
pub mod tree;

pub use allowlist::*;
pub use boundaries::*;
pub use code::*;
pub use comments::*;
pub use figures::*;
pub use finding::*;
pub use functions::*;
pub use history::*;
pub use items::*;
pub use modules::*;
pub use order::*;
pub use primary::*;
pub use runner::*;
pub use scope::*;
pub use summaries::*;
pub use tree::*;
