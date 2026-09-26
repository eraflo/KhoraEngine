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

//! Native functions: their descriptors, their registration, and the
//! registry that finds them.

use super::{builtins, NativeTy};

use super::context::{NativeContext, NativeError};
use crate::vm::Value;

/// A function the engine exposes to scripts.
pub struct NativeFn {
    /// The name a script calls it by.
    pub name: &'static str,
    /// What it takes.
    ///
    /// The exact list, unless [`variadic`](Self::variadic) — then the minimum.
    pub params: &'static [NativeTy],
    /// What it gives back.
    pub result: NativeTy,
    /// What one call costs against the frame's fuel.
    pub cost: u64,
    /// Whether it accepts arguments beyond the declared ones.
    ///
    /// Almost nothing should. It exists for the calls whose payload the *callee*
    /// defines rather than the caller — `Raise(e, "Damaged", 10)` carries
    /// whatever the handler declares, and the checker cannot know which handler
    /// that will be, because the answer depends on the state the entity is in
    /// when the event arrives. Those arguments are therefore checked at
    /// delivery, where the handler is finally known, rather than pretended to be
    /// checked here.
    ///
    /// A native written with `#[ergon_fn]` is never variadic: the macro derives
    /// its signature from typed Rust parameters, which is the whole point.
    pub variadic: bool,
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
    /// **This is the registry the engine runs on.** The compiler, the type
    /// checker and the [`Host`](super::Host) all take it, which is not a convenience: a
    /// program addresses a native by index, so a registry assembled differently
    /// on two sides would resolve the same call to two different functions.
    ///
    /// The submissions are sorted by name before being added. `inventory` yields
    /// them in link order, which is not stable across builds — and an unstable
    /// order would make a call mean one function today and its neighbour after a
    /// relink. Sorting costs one pass and removes the whole class.
    ///
    /// The discovery itself happens once. What each caller pays is a vector of
    /// pointers, not another walk of the submissions.
    pub fn discovered() -> Self {
        static DISCOVERED: std::sync::OnceLock<Vec<&'static NativeFn>> = std::sync::OnceLock::new();

        let functions = DISCOVERED.get_or_init(|| {
            let mut registry = Self::with_builtins();

            let mut submitted: Vec<&'static NativeFn> = inventory::iter::<NativeRegistration>
                .into_iter()
                .map(|registration| registration.0)
                .collect();
            submitted.sort_by_key(|function| function.name);

            for function in submitted {
                registry.register(function);
            }
            registry.functions
        });

        Self {
            functions: functions.clone(),
        }
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

    /// Whether none are exposed.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many functions are exposed.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Every function, in index order.
    pub fn iter(&self) -> impl Iterator<Item = &'static NativeFn> + '_ {
        self.functions.iter().copied()
    }
}
