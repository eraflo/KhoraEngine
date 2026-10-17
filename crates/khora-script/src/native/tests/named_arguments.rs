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

//! Named arguments at a host's hand-written native: a variadic one takes its
//! arguments in order, whether it is called by name or on an entity.

use super::{errors, NativeContext, NativeFn, NativeRegistry, NativeTy};
use crate::vm::Value;

/// `Tag(Entity, string, ...)`: variadic, its first parameter an entity, so a
/// script calls it on one — `e.Tag("boss", …)`.
static TAG: NativeFn = NativeFn {
    name: "Tag",
    params: &[NativeTy::Entity, NativeTy::Str],
    names: &["entity", "label"],
    result: NativeTy::Void,
    cost: 1,
    variadic: true,
    call: |_: &mut NativeContext<'_>, _| Ok(Value::Unit),
};

/// **A variadic native takes no named argument, called on an entity too.**
/// The free form says so; the method form must not drop the name silently and
/// pass the value by position.
#[test]
fn a_variadic_native_called_on_an_entity_takes_no_named_argument() {
    let mut natives = NativeRegistry::new();
    natives.register(&TAG);

    let found = errors(
        "fn void Main(Entity e) { e.Tag(label: \"boss\"); }",
        &natives,
    );
    assert!(
        found.iter().any(|m| m.contains("takes no named argument")),
        "the label is refused, not dropped: {found:?}"
    );
}
