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

//! The engine functions a script may call.
//!
//! # The surface is a list, not a consequence
//!
//! A script can call exactly what is registered here and nothing else. That is
//! the design, not a limitation waiting to be lifted: a scripting language whose
//! reach is "whatever the host happens to export" has a surface nobody decided
//! and nobody can review. Adding a function is a deliberate act, and its
//! signature sits next to it as a constant so the two cannot drift.
//!
//! # Fuel is per-function, not per-instruction
//!
//! Every VM instruction costs the same small amount, which is right for
//! arithmetic and wrong for a native: a raycast is not a `Move`. So a native
//! declares its own [`cost`](NativeFn::cost), and the budget the DCC handed out
//! is spent at the rate the work actually takes. Without that, a behavior could
//! burn a frame inside one call while the fuel counter reported it had barely
//! started.
//!
//! # What a native can reach
//!
//! [`NativeContext`] — and deliberately not the `World`. A native that changes
//! the world queues a [`WorldCommand`] like everything else, because the lane it
//! runs inside may not write (`RULES.md` §3). Natives that *read* engine state
//! arrive with the script lane, which is what has a projected view to read from.
//!
//! [`WorldCommand`]: khora_core::script::WorldCommand

pub mod builtins;
pub mod convert;
pub mod ty;

#[cfg(test)]
mod tests;

pub use builtins::builtins;
pub use convert::ScriptType;
pub use ty::NativeTy;

use khora_core::ecs::entity::EntityId;
use khora_core::script::CommandBuffer;

use crate::arena::Arena;
use crate::vm::{StrError, Value};

/// Why a native call failed.
///
/// A native faults the calling behavior; it never takes the process down. The
/// message is the one an author reads, so it names the call rather than the
/// internal condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeError {
    /// What went wrong.
    pub message: String,
}

impl NativeError {
    /// Reports a failure.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// What a native call is allowed to touch.
///
/// Borrowed for the duration of one call rather than owned, so a native cannot
/// keep any of it past the frame that lent it.
pub struct NativeContext<'a> {
    /// Where effects on the world are queued.
    pub commands: &'a mut CommandBuffer,
    /// Frame memory, for arrays and strings.
    pub arena: &'a mut Arena,
    /// The entity the calling behavior is attached to.
    ///
    /// `None` while running a free function that no entity owns — a native that
    /// needs a subject must say so rather than assume one.
    pub entity: Option<EntityId>,
    /// The running program's string literals.
    ///
    /// Needed because a string argument can live in either the program or the
    /// arena, and the value only says which — so resolving one needs both, and
    /// a native that takes a `string` would otherwise have no way to read it.
    pub strings: &'a [String],
}

impl NativeContext<'_> {
    /// The text a string value stands for.
    pub fn string(&self, value: Value) -> Result<&str, NativeError> {
        crate::vm::resolve_str(value, self.strings, self.arena).map_err(|error| match error {
            StrError::NotAString(found) => NativeError::new(format!(
                "an engine function expected a string but the call supplied {found}"
            )),
            StrError::NotInProgram => NativeError::new("this text is not in the running program"),
            StrError::Gone => NativeError::new(
                "this text was made in an earlier frame and no longer exists — \
                 to keep one, put it in a behavior field",
            ),
        })
    }
}

/// Everything a running program needs from outside itself.
///
/// Owns what [`NativeContext`] only borrows, so a caller holds one of these for
/// a frame rather than assembling the pieces at every [`run`] — and so the
/// commands a script queued are still there afterwards to be applied.
///
/// [`run`]: crate::vm::Machine::run
#[derive(Debug)]
pub struct Host {
    /// What the program may call.
    pub natives: NativeRegistry,
    /// Effects queued for the frame boundary.
    pub commands: CommandBuffer,
    /// Frame memory.
    pub arena: Arena,
    /// The entity the running behavior belongs to.
    pub entity: Option<EntityId>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    /// A host exposing the language's built-ins and nothing else.
    pub fn new() -> Self {
        Self {
            natives: NativeRegistry::with_builtins(),
            commands: CommandBuffer::new(),
            arena: Arena::new(),
            entity: None,
        }
    }

    /// A host exposing nothing at all, for a program that must call nothing.
    pub fn bare() -> Self {
        Self {
            natives: NativeRegistry::new(),
            ..Self::new()
        }
    }

    /// Names the entity the running behavior belongs to.
    pub fn for_entity(mut self, entity: EntityId) -> Self {
        self.entity = Some(entity);
        self
    }

    /// Ends the frame: the arena is freed and the queued commands handed over.
    ///
    /// Both in one step because they belong to the same moment — releasing the
    /// arena while the commands still referred to it is exactly the mistake the
    /// generation counter exists to catch, and doing it here means no caller has
    /// to remember the order.
    pub fn end_frame(&mut self) -> CommandBuffer {
        self.arena.reset();
        std::mem::take(&mut self.commands)
    }

    /// The narrow view a native gets.
    ///
    /// `strings` comes from the program rather than the host: the same host
    /// runs whatever program the frame hands it, and a string constant belongs
    /// to the program it was compiled into.
    pub fn context<'a>(&'a mut self, strings: &'a [String]) -> NativeContext<'a> {
        NativeContext {
            commands: &mut self.commands,
            arena: &mut self.arena,
            entity: self.entity,
            strings,
        }
    }
}

/// A function the engine exposes to scripts.
pub struct NativeFn {
    /// The name a script calls it by.
    pub name: &'static str,
    /// What it takes.
    pub params: &'static [NativeTy],
    /// What it gives back.
    pub result: NativeTy,
    /// What one call costs against the frame's fuel.
    pub cost: u64,
    /// The implementation.
    pub call: fn(&mut NativeContext<'_>, &[Value]) -> Result<Value, NativeError>,
}

impl std::fmt::Debug for NativeFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeFn")
            .field("name", &self.name)
            .field("params", &self.params)
            .field("result", &self.result)
            .field("cost", &self.cost)
            .finish_non_exhaustive()
    }
}

/// A function `#[ergon_fn]` has submitted.
///
/// A newtype so the collection has something to name; the macro emits one of
/// these next to the function it annotates.
pub struct NativeRegistration(pub &'static NativeFn);

inventory::collect!(NativeRegistration);

/// The natives a program may call, in a fixed order.
///
/// Order is part of the contract: compiled bytecode addresses a native by
/// index, so a program and the registry it was compiled against have to agree.
/// Building the registry once and handing it to both the compiler and the VM is
/// what keeps them in step — a registry assembled differently on the two sides
/// would resolve `Log` to whatever now sits at that index.
#[derive(Debug, Default)]
pub struct NativeRegistry {
    functions: Vec<&'static NativeFn>,
}

impl NativeRegistry {
    /// An empty registry — a script can call nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry every program gets: the language's own built-ins.
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        for function in builtins() {
            registry.register(function);
        }
        registry
    }

    /// The built-ins plus everything `#[ergon_fn]` has registered.
    ///
    /// The submissions are sorted by name before being added. `inventory`
    /// yields them in link order, which is not stable across builds — and a
    /// program addresses a native by index, so an unstable order would make a
    /// call mean one function today and its neighbour after a relink. Sorting
    /// costs one pass at start-up and removes the whole class.
    pub fn discovered() -> Self {
        let mut registry = Self::with_builtins();

        let mut submitted: Vec<&'static NativeFn> = inventory::iter::<NativeRegistration>
            .into_iter()
            .map(|registration| registration.0)
            .collect();
        submitted.sort_by_key(|function| function.name);

        for function in submitted {
            registry.register(function);
        }
        registry
    }

    /// Adds a function, returning its index.
    ///
    /// A name already present is replaced in place rather than shadowed, so the
    /// index of everything else is unchanged and a program compiled earlier
    /// still addresses what it meant.
    pub fn register(&mut self, function: &'static NativeFn) -> usize {
        match self.index_of(function.name) {
            Some(index) => {
                self.functions[index] = function;
                index
            }
            None => {
                self.functions.push(function);
                self.functions.len() - 1
            }
        }
    }

    /// The index a name resolves to.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.functions.iter().position(|f| f.name == name)
    }

    /// The function a name resolves to.
    pub fn get(&self, name: &str) -> Option<&'static NativeFn> {
        self.index_of(name).map(|index| self.functions[index])
    }

    /// The function at an index.
    pub fn at(&self, index: usize) -> Option<&'static NativeFn> {
        self.functions.get(index).copied()
    }

    /// How many functions are exposed.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether nothing is exposed.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Every function, in index order.
    pub fn iter(&self) -> impl Iterator<Item = &'static NativeFn> + '_ {
        self.functions.iter().copied()
    }
}
