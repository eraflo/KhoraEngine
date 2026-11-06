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

//! The compile family: a project's scripts recompiled off the frame.
//!
//! The hot-reload pump files a [`CompileRequest`] for each change; the
//! family's lane, at `ExecutionPhase::BACKGROUND`, takes them and hands them
//! to the runtime's [`ScriptCompiler`], and stages what it returns — the
//! recompiled programs and what their modules declare — for a frame's
//! boundary. A family of its own, not the scripting one: it runs while the
//! scripts are off, as an editor editing a scene needs.
//!
//! [`CompileRequest`]: khora_script::toolchain::CompileRequest
//! [`ScriptCompiler`]: khora_script::toolchain::ScriptCompiler

mod compile;
mod compile_family;

pub use compile::*;
pub use compile_family::*;
