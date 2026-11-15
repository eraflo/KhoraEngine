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

//! Where a span of a compiled program sits: its module, line and column.
//!
//! Each module of a program has a base in one program-wide offset space, so a
//! [`Span`] alone does not say which file it points into.

use crate::diagnostics::{SourceFile, Span};

/// A position in one module: 1-based line and column, the column counted in
/// characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The module, by its normalised path.
    pub module: String,
    /// 1-based line.
    pub line: u32,
    /// 1-based column, in characters.
    pub column: u32,
}

/// Every module of a compiled program, by where it sits in the program's
/// offset space.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Each module's source and ascending base, a gap of one between neighbours
    /// so a span at the end of one never reads as the start of the next.
    modules: Vec<(u32, SourceFile)>,
}

impl SourceMap {
    /// Places `source` after every module placed so far, returning the base
    /// its spans are offset by.
    pub fn place(&mut self, source: SourceFile) -> u32 {
        let base = self
            .modules
            .last()
            .map_or(0, |(base, last)| base + last.text().len() as u32 + 1);
        self.modules.push((base, source));
        base
    }

    /// Returns the module, line and column `span` starts at; `None` when it
    /// falls in no module of the program.
    pub fn locate(&self, span: Span) -> Option<Location> {
        let (base, source) = self
            .modules
            .iter()
            .rev()
            .find(|(base, _)| *base <= span.start)?;
        let offset = span.start - base;
        if offset as usize > source.text().len() {
            return None;
        }
        let (line, column) = source.line_col(offset);
        Some(Location {
            module: source.name().to_owned(),
            line,
            column,
        })
    }
}
