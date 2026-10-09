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

//! A saved boundary value converted into a slot of a declared struct type.

use khora_core::script::ScriptValue;
use khora_script::arena::Owned;
use khora_script::bridge::to_owned_as;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

fn record(fields: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

const PAIR: &str = "struct Pair { int a; float f = 1.5; Entity? who = null; }
                    fn int G() { return 0; }";

#[test]
fn an_array_of_structs_with_an_element_field_retyped_does_not_load() {
    let program = build(PAIR);
    let saved = ScriptValue::Array(vec![
        record(&[("a", ScriptValue::Int(1))]),
        record(&[("a", ScriptValue::Str("one".to_owned()))]),
    ]);
    assert!(to_owned_as(&saved, "Pair[]", &program.structs).is_err());
}

#[test]
fn a_saved_int_loads_into_a_float_struct_field_and_a_null_into_an_optional_entity() {
    let program = build(PAIR);
    let saved = record(&[
        ("a", ScriptValue::Int(1)),
        ("f", ScriptValue::Int(2)),
        ("who", ScriptValue::Null),
    ]);
    let owned = to_owned_as(&saved, "Pair", &program.structs).expect("it fits");
    assert_eq!(
        owned,
        Owned::Struct {
            name: "Pair".to_owned(),
            fields: vec![
                ("a".to_owned(), Owned::Scalar(Value::Int(1))),
                ("f".to_owned(), Owned::Scalar(Value::Int(2))),
                ("who".to_owned(), Owned::Scalar(Value::Null)),
            ],
        }
    );
}
