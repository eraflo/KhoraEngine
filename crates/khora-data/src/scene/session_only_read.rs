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

//! What a load does with a value only a session holds, read from its record.

use super::apply::Loaded;
use super::component_registration::StagedComponent;

/// What a load does with a value only a session holds, read from its record.
///
/// Which values those are, a registration says:
/// [`ComponentRegistration::session_only`](super::ComponentRegistration::session_only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SessionOnlyRead {
    /// Refused: a scene or a game save names what no file holds.
    ///
    /// The value is dropped from its entity and reported
    /// [`ReportKind::Dropped`](super::record::ReportKind::Dropped).
    Refused,
    /// Kept: a snapshot is read back in the session that wrote it, and a copy
    /// made within the session never left it.
    Kept,
}

impl SessionOnlyRead {
    /// Returns whether the load refuses `read`, a value of `loaded`: it lives
    /// only in a session, and this load refuses such values.
    pub(super) fn refuses(self, loaded: &Loaded, read: &StagedComponent) -> bool {
        self == Self::Refused
            && match loaded {
                Loaded::Rust(reg) => reg
                    .session_only
                    .is_some_and(|holds| read.component.is_session_only(holds)),
                Loaded::Declared(_) => false,
            }
    }
}
