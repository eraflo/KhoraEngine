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

//! The tracing deserializer behind [`schema_of`](super::schema_of).
//!
//! A deserializer that has no input: every value a type asks for is answered
//! with a placeholder, and the request itself — a struct with these fields,
//! a sequence of this — is written down. One run visits one variant of each
//! enum it meets, so the type is run again until every variant has been
//! visited.
//!
//! A type may refuse a placeholder — a UUID wants sixteen bytes, not none.
//! The run that met the refusal answers that place differently the next
//! time, until the type takes one, so no field is left out of the trace.
//!
//! A recursive type would run forever. A container met again while it is
//! being traced is read once more in **shadow**: nothing is written down,
//! every option answers `None`, every sequence and map is empty, every enum
//! takes a variant known to end — the smallest value the type has, so the
//! trace goes on past it and every field after it is traced. Only a type
//! with no such value (one that always contains itself) stops the run; an
//! option or a map holding one is answered `None` or empty on the next run.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};

use khora_core::script::frozen::SUSPENDED_MACHINE_NAME;
use serde::de::{self, DeserializeOwned, Visitor};

use super::access::{Elements, Entry, One, Variant};
use super::{Container, Fields, Format, Key, Payload};

/// How many runs a type may take. Each run learns something no earlier run
/// did, or the trace ends; this only bounds a pathological type.
const MAX_RUNS: usize = 4096;

/// The owner of a tuple's elements, of a sequence's, of a map's entries.
const TUPLE: Key = ("tuple", "");
const SEQ: Key = ("seq", "");
pub(super) const MAP: Key = ("map", "");

/// Why a run stopped.
#[derive(Debug)]
pub(super) enum TraceError {
    /// A container that always contains itself was met again.
    Recursion,
    /// The type refused a placeholder.
    Message(String),
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recursion => f.write_str("the type contains itself"),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TraceError {}

impl de::Error for TraceError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Message(message.to_string())
    }
}

/// Where a value sits: the path from the root to it — each container, the
/// variant if any, the position — folded into one number. The same in
/// every run, whatever the variants chosen, and different for any two
/// places.
pub(super) type Slot = u64;

/// The slot of position `index` of `owner` / `variant` under `parent`.
pub(super) fn child(parent: Slot, owner: Key, variant: &'static str, index: usize) -> Slot {
    let mut hasher = DefaultHasher::new();
    (parent, owner, variant, index).hash(&mut hasher);
    hasher.finish()
}

/// What the runs of one trace have learned.
#[derive(Default)]
pub(super) struct State {
    containers: BTreeMap<Key, Container>,
    pub(super) enums: HashMap<Key, Progress>,
    /// Options answered `None`: a `Some` there always recursed. The inner
    /// format it was seen with.
    none_at: HashMap<Slot, Format>,
    /// Maps answered empty: a value there always recursed. Their key and
    /// value formats.
    empty_at: HashMap<Slot, (Format, Format)>,
    /// Places a placeholder was refused at, and which one to answer next.
    placeholders: HashMap<Slot, usize>,
}

impl State {
    /// A measure that grows whenever a run learns something.
    fn learned(&self) -> usize {
        self.containers.len()
            + self.none_at.len()
            + self.empty_at.len()
            + self.placeholders.values().sum::<usize>()
            + self
                .enums
                .values()
                .map(|progress| {
                    progress.payloads.iter().flatten().count()
                        + progress.failed.len()
                        + progress.retried.len()
                })
                .sum::<usize>()
    }

    /// Whether some variant is still worth a run.
    fn pending(&self) -> bool {
        self.enums
            .values()
            .any(|progress| progress.next_untraced().is_some())
    }
}

/// One enum's variants, and what is known of each.
pub(super) struct Progress {
    pub(super) variants: &'static [&'static str],
    pub(super) payloads: Vec<Option<Payload>>,
    /// Variants whose payload recursed with no way to end.
    failed: HashSet<usize>,
    /// Failed variants tried again once another variant was known to end.
    retried: HashSet<usize>,
}

impl Progress {
    fn new(variants: &'static [&'static str]) -> Self {
        Self {
            variants,
            payloads: vec![None; variants.len()],
            failed: HashSet::new(),
            retried: HashSet::new(),
        }
    }

    /// A variant traced to the end: one an enum met inside itself can take.
    fn ending(&self) -> Option<usize> {
        self.payloads.iter().position(Option::is_some)
    }

    /// The next variant no run has traced yet, if one is worth a try.
    fn next_untraced(&self) -> Option<usize> {
        let untraced = |index: &usize| self.payloads[*index].is_none();
        (0..self.variants.len())
            .filter(untraced)
            .find(|index| !self.failed.contains(index))
            .or_else(|| {
                self.ending()?;
                (0..self.variants.len())
                    .filter(untraced)
                    .find(|index| !self.retried.contains(index))
            })
    }
}

/// Traces `T`: its root format, every container it holds, and whether every
/// value was traced.
pub(super) fn trace<T: DeserializeOwned>() -> (Format, BTreeMap<Key, Container>, bool) {
    let mut state = State::default();
    let mut root = Format::Unknown;
    let mut complete = false;
    for _ in 0..MAX_RUNS {
        let before = state.learned();
        let mut tracer = Tracer {
            state: &mut state,
            open: Vec::new(),
            shadow: 0,
            slot: 0,
            out: Format::Unknown,
            answered: None,
        };
        let result = T::deserialize(&mut tracer);
        root = tracer.out;
        let answered = tracer.answered;
        if let (Err(TraceError::Message(_)), Some((slot, choices))) = (&result, answered) {
            // The type refused what was answered last: answer it otherwise.
            let next = state.placeholders.entry(slot).or_insert(0);
            if *next + 1 < choices {
                *next += 1;
            }
        }
        complete = result.is_ok();
        if complete && !state.pending() {
            break;
        }
        if state.learned() == before {
            break;
        }
    }
    let mut containers = state.containers;
    for (key, progress) in state.enums {
        let variants = progress
            .variants
            .iter()
            .copied()
            .zip(progress.payloads)
            .collect();
        containers.insert(key, Container::Enum(variants));
    }
    (root, containers, complete)
}

/// One run.
pub(super) struct Tracer<'s> {
    pub(super) state: &'s mut State,
    /// The containers being traced, outermost first.
    open: Vec<Key>,
    /// How many containers deep the run is reading in shadow — a container
    /// met again inside itself, read for its smallest value, written down
    /// nowhere.
    pub(super) shadow: usize,
    /// Where the value being read sits.
    pub(super) slot: Slot,
    /// The format of the value last read.
    pub(super) out: Format,
    /// The last placeholder answered: where, and how many it could have been.
    answered: Option<(Slot, usize)>,
}

/// The placeholder lengths a byte string is answered with, in turn: none,
/// then the lengths fixed-size values are made of.
const BYTE_LENGTHS: [usize; 9] = [0, 16, 32, 8, 4, 12, 20, 64, 1];

/// How a container was entered.
#[derive(Clone, Copy)]
enum Entered {
    /// Traced, and written down.
    Traced,
    /// Met again inside itself: read in shadow.
    Shadow,
}

impl Tracer<'_> {
    /// Answers a primitive here: records its format and says which of its
    /// `choices` placeholders to give.
    fn primitive(&mut self, name: &'static str, choices: usize) -> usize {
        self.out = Format::Primitive(name);
        self.answered = Some((self.slot, choices));
        self.state
            .placeholders
            .get(&self.slot)
            .copied()
            .unwrap_or(0)
            .min(choices - 1)
    }

    /// Reads `count` elements of `owner` / `variant`, each in its own slot
    /// under this one, their formats in order.
    pub(super) fn elements<'de, V: Visitor<'de>>(
        &mut self,
        owner: Key,
        variant: &'static str,
        count: usize,
        visitor: V,
    ) -> (Result<V::Value, TraceError>, Vec<Format>) {
        let mut formats = Vec::with_capacity(count);
        let parent = self.slot;
        let result = visitor.visit_seq(Elements {
            tracer: self,
            parent,
            owner,
            variant,
            next: 0,
            count,
            formats: &mut formats,
        });
        (result, formats)
    }

    /// Opens `key`: traced if it is not open, in shadow if it is — and a
    /// recursion if it is already open in shadow, a type with no value that
    /// ends.
    fn enter(&mut self, key: Key) -> Result<Entered, TraceError> {
        if !self.open.contains(&key) {
            self.open.push(key);
            return Ok(Entered::Traced);
        }
        if self.shadow > 0 {
            self.out = Format::Named(key);
            return Err(TraceError::Recursion);
        }
        self.shadow += 1;
        Ok(Entered::Shadow)
    }

    fn leave(&mut self, key: Key, entered: Entered, container: Option<Container>) {
        match entered {
            Entered::Traced => {
                self.open.pop();
                if let Some(container) = container {
                    if self.shadow == 0 {
                        self.state.containers.insert(key, container);
                    }
                }
            }
            Entered::Shadow => self.shadow -= 1,
        }
        self.out = Format::Named(key);
    }

    /// Runs `read` with the slot moved to `slot`.
    pub(super) fn at<R>(&mut self, slot: Slot, read: impl FnOnce(&mut Self) -> R) -> R {
        let parent = std::mem::replace(&mut self.slot, slot);
        let result = read(self);
        self.slot = parent;
        result
    }
}

/// A container's identity: its serde name and the visitor reading it.
fn key<V>(name: &'static str) -> Key {
    (name, std::any::type_name::<V>())
}

impl<'de> de::Deserializer<'de> for &mut Tracer<'_> {
    type Error = TraceError;

    fn is_human_readable(&self) -> bool {
        false
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        self.primitive("any", 1);
        visitor.visit_unit()
    }
    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("bool", 2);
        visitor.visit_bool(choice == 1)
    }
    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("i8", 3);
        visitor.visit_i8([0, 1, -1][choice])
    }
    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("i16", 3);
        visitor.visit_i16([0, 1, -1][choice])
    }
    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("i32", 3);
        visitor.visit_i32([0, 1, -1][choice])
    }
    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("i64", 3);
        visitor.visit_i64([0, 1, -1][choice])
    }
    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("u8", 3);
        visitor.visit_u8([0, 1, 255][choice])
    }
    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("u16", 3);
        visitor.visit_u16([0, 1, 2][choice])
    }
    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("u32", 3);
        visitor.visit_u32([0, 1, 2][choice])
    }
    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("u64", 3);
        visitor.visit_u64([0, 1, 2][choice])
    }
    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("f32", 2);
        visitor.visit_f32([0.0, 1.0][choice])
    }
    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("f64", 2);
        visitor.visit_f64([0.0, 1.0][choice])
    }
    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("char", 2);
        visitor.visit_char(['a', '0'][choice])
    }
    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("string", 3);
        visitor.visit_str(["", "a", "0"][choice])
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        self.deserialize_str(visitor)
    }
    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let choice = self.primitive("bytes", BYTE_LENGTHS.len());
        visitor.visit_byte_buf(vec![0; BYTE_LENGTHS[choice]])
    }
    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        self.deserialize_bytes(visitor)
    }
    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        self.primitive("unit", 1);
        visitor.visit_unit()
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        visitor.visit_u32(0)
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        visitor.visit_unit()
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let slot = self.slot;
        if self.shadow > 0 {
            self.out = Format::Option(Box::new(Format::Unknown));
            return visitor.visit_none();
        }
        if let Some(inner) = self.state.none_at.get(&slot) {
            self.out = Format::Option(Box::new(inner.clone()));
            return visitor.visit_none();
        }
        let result = self.at(child(slot, ("option", ""), "", 0), |tracer| {
            visitor.visit_some(&mut *tracer)
        });
        let inner = std::mem::replace(&mut self.out, Format::Unknown);
        if matches!(result, Err(TraceError::Recursion)) {
            // The next run answers `None` here, with what this one saw.
            self.state.none_at.insert(slot, inner.clone());
        }
        self.out = Format::Option(Box::new(inner));
        result
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let mut element = Format::Unknown;
        let slot = child(self.slot, SEQ, "", 0);
        let done = self.shadow > 0;
        let result = visitor.visit_seq(One {
            tracer: &mut *self,
            slot,
            done,
            format: &mut element,
        });
        self.out = Format::Seq(Box::new(element));
        result
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let (result, formats) = self.elements(TUPLE, "", len, visitor);
        self.out = Format::Tuple(formats);
        result
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, TraceError> {
        let slot = self.slot;
        let empty = if self.shadow > 0 {
            Some((Format::Unknown, Format::Unknown))
        } else {
            self.state.empty_at.get(&slot).cloned()
        };
        if let Some((key, value)) = empty {
            let result = visitor.visit_map(Entry {
                tracer: &mut *self,
                slot,
                done: true,
                key: &mut Format::Unknown,
                value: &mut Format::Unknown,
            });
            self.out = Format::Map(Box::new(key), Box::new(value));
            return result;
        }
        let (mut key, mut value) = (Format::Unknown, Format::Unknown);
        let result = visitor.visit_map(Entry {
            tracer: &mut *self,
            slot,
            done: false,
            key: &mut key,
            value: &mut value,
        });
        if matches!(result, Err(TraceError::Recursion)) {
            // The next run answers an empty map here, with what this one saw.
            self.state
                .empty_at
                .insert(slot, (key.clone(), value.clone()));
        }
        self.out = Format::Map(Box::new(key), Box::new(value));
        result
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let key = key::<V>(name);
        if self.shadow == 0 {
            self.state.containers.insert(key, Container::Unit);
        }
        self.out = Format::Named(key);
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        if name == SUSPENDED_MACHINE_NAME {
            // The reserved name only announces the enum of a machine's two
            // forms, which the visitor reads next: nothing of its own.
            return visitor.visit_newtype_struct(self);
        }
        let key = key::<V>(name);
        let entered = self.enter(key)?;
        let slot = child(self.slot, key, "", 0);
        let result = self.at(slot, |tracer| visitor.visit_newtype_struct(&mut *tracer));
        let inner = std::mem::replace(&mut self.out, Format::Unknown);
        let traced = result.is_ok().then(|| Container::Tuple(vec![inner]));
        self.leave(key, entered, traced);
        result
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let key = key::<V>(name);
        let entered = self.enter(key)?;
        let (result, formats) = self.elements(key, "", len, visitor);
        let traced = result.is_ok().then_some(Container::Tuple(formats));
        self.leave(key, entered, traced);
        result
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let key = key::<V>(name);
        let entered = self.enter(key)?;
        // `fields` lists the names a struct is read under — older ones
        // included — so the struct may read fewer elements than it lists.
        let (result, formats) = self.elements(key, "", fields.len(), visitor);
        let traced = result.is_ok().then_some(Container::Struct(Fields {
            names: fields,
            formats,
        }));
        self.leave(key, entered, traced);
        result
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, TraceError> {
        let key = key::<V>(name);
        let inside_itself = self.open.contains(&key) || self.shadow > 0;
        let progress = self
            .state
            .enums
            .entry(key)
            .or_insert_with(|| Progress::new(variants));
        let chosen = if inside_itself {
            progress
                .ending()
                .or_else(|| (self.shadow > 0 && !variants.is_empty()).then_some(0))
        } else {
            progress
                .next_untraced()
                .or_else(|| progress.ending())
                .or((!variants.is_empty()).then_some(0))
        };
        let Some(index) = chosen else {
            self.out = Format::Named(key);
            return Err(TraceError::Recursion);
        };
        if !inside_itself && progress.failed.contains(&index) {
            progress.retried.insert(index);
        }
        if !inside_itself {
            self.open.push(key);
        }
        let result = visitor.visit_enum(Variant {
            tracer: &mut *self,
            key,
            index,
            record: !inside_itself,
        });
        if !inside_itself {
            self.open.pop();
            if result.is_err() {
                if let Some(progress) = self.state.enums.get_mut(&key) {
                    if progress.payloads[index].is_none() {
                        progress.failed.insert(index);
                    }
                }
            }
        }
        self.out = Format::Named(key);
        result
    }
}
