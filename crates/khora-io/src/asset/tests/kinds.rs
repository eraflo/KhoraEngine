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

//! The asset kinds the crate's unit tests declare, each named like the extension of its files.

khora_core::register_asset_kind!(
    /// Test container files (`*.ksubtest`), listed line by line, which no production lister claims.
    pub(crate) const TEST_CONTAINER = "ksubtest"
);

khora_core::register_asset_kind!(
    /// Test container files whose lister always fails (`*.ksubfail`).
    pub(crate) const FAILING_CONTAINER = "ksubfail"
);

khora_core::register_asset_kind!(
    /// Test container files packed by the pack builder's tests (`*.ksubpack`).
    pub(crate) const PACK_CONTAINER = "ksubpack"
);

khora_core::register_asset_kind!(
    /// Plain test files, read as their text (`*.ktestnote`).
    pub(crate) const NOTE = "ktestnote"
);
