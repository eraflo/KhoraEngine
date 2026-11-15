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

//! What a developer asks of a save: a [`SerializationGoal`], not a file format.

/// What a serialization is for; the serialization service picks the encoding that serves it.
///
/// The goal changes the encoding, never what is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SerializationGoal {
    /// The fastest load, served by a snapshot bound to the build that wrote it.
    ///
    /// A same-build cache, never a project's only copy. A world holding a
    /// component whose schema cannot be fingerprinted is written as a compact
    /// record instead.
    FastestLoad,

    /// The smallest file, served by the compact encoding.
    SmallestFileSize,

    /// A file people can read and diff, as JSON; the slowest option.
    HumanReadableDebug,

    /// A file readable without Khora for years, as JSON.
    ///
    /// Every encoding holds components and fields by name, so future versions
    /// of Khora read them all.
    LongTermStability,

    /// A structured format for editor operations, prefabs and tool interchange.
    EditorInterchange,

    /// A portable, schema-less binary format (MessagePack), for non-Rust tools.
    ///
    /// Slightly larger than `EditorInterchange`, but readable by every
    /// language with a MessagePack library.
    PortableBinary,
}
