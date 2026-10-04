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

//! Compile-level guard over `khora_script`'s public surface, the paths `#[ergon_fn]`
//! emits, plus the paths
//! other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), and the
//! trait impls. A reorganisation that moves code between files must keep every
//! one of these paths valid, so this module stops compiling the moment one
//! disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions).
//!
//! The list was generated from `khora_script`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.
//!
//! Some paths are also written by `#[ergon_fn]` into the crate that uses it
//! (`khora-macros/src/ergon_fn.rs`); they are pinned below with the leading
//! `::` of the expansion, and the `ergon_fn` module expands the attribute from
//! outside the crate.

use std::any::type_name;
use std::marker::PhantomData;

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

fn is_clone<T: Clone>() {}
fn is_collect<T: inventory::Collect>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_deserialize_owned<T: serde::de::DeserializeOwned>() {}
fn is_display<T: std::fmt::Display>() {}
fn is_eq<T: Eq>() {}
fn is_hash<T: std::hash::Hash>() {}
fn is_ord<T: Ord>() {}
fn is_partial_eq<T: PartialEq>() {}
fn is_partial_ord<T: PartialOrd>() {}
fn is_serialize<T: serde::Serialize>() {}
fn is_source_loader<T: khora_script::modules::loader::SourceLoader>() {}
fn is_supersedes<T: khora_core::event::Supersedes>() {}

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_script::arena as _;
    use khora_script::arena::persistent as _;
    use khora_script::ast as _;
    use khora_script::ast::decl as _;
    use khora_script::ast::expr as _;
    use khora_script::ast::stmt as _;
    use khora_script::ast::types as _;
    use khora_script::bridge as _;
    use khora_script::bytecode as _;
    use khora_script::bytecode::expr as _;
    use khora_script::bytecode::registers as _;
    use khora_script::bytecode::stmt as _;
    use khora_script::diagnostics as _;
    use khora_script::dispatch as _;
    use khora_script::lexer as _;
    use khora_script::lexer::scanner as _;
    use khora_script::lexer::token as _;
    use khora_script::lifecycle as _;
    use khora_script::modules as _;
    use khora_script::modules::loader as _;
    use khora_script::modules::path as _;
    use khora_script::native as _;
    use khora_script::native::builtins as _;
    use khora_script::native::convert as _;
    use khora_script::native::engine_types as _;
    use khora_script::native::events as _;
    use khora_script::native::input as _;
    use khora_script::native::ty as _;
    use khora_script::native::world as _;
    use khora_script::parser as _;
    use khora_script::pipeline as _;
    use khora_script::reload as _;
    use khora_script::types as _;
    use khora_script::types::expr as _;
    use khora_script::types::scope as _;
    use khora_script::types::stmt as _;
    use khora_script::types::ty as _;
    use khora_script::vm as _;
    use khora_script::vm::instruction as _;
    use khora_script::vm::program as _;
    use khora_script::vm::value as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn arena_error_variants(x: &khora_script::arena::ArenaError) {
    match x {
        khora_script::arena::ArenaError::Stale { .. } => {}
        khora_script::arena::ArenaError::OutOfBounds => {}
        khora_script::arena::ArenaError::Full => {}
    }
}

fn object_variants(x: &khora_script::arena::Object) {
    match x {
        khora_script::arena::Object::Array(..) => {}
        khora_script::arena::Object::Str(..) => {}
    }
}

fn persisted_variants(x: &khora_script::arena::persistent::Persisted) {
    match x {
        khora_script::arena::persistent::Persisted::Scalar(..) => {}
        khora_script::arena::persistent::Persisted::Owned(..) => {}
    }
}

fn item_variants(x: &khora_script::ast::Item) {
    match x {
        khora_script::ast::Item::Struct(..) => {}
        khora_script::ast::Item::Function(..) => {}
        khora_script::ast::Item::Behavior(..) => {}
    }
}

fn module_fields(x: &khora_script::ast::Module) {
    let _ = (&x.imports, &x.items);
}

fn after_decl_fields(x: &khora_script::ast::decl::AfterDecl) {
    let _ = (&x.delay, &x.body, &x.span);
}

fn attribute_fields(x: &khora_script::ast::decl::Attribute) {
    let _ = (&x.name, &x.args, &x.span);
}

fn behavior_decl_fields(x: &khora_script::ast::decl::BehaviorDecl) {
    let _ = (&x.attributes, &x.name, &x.name_span, &x.members, &x.span);
}

fn behavior_member_variants(x: &khora_script::ast::decl::BehaviorMember) {
    match x {
        khora_script::ast::decl::BehaviorMember::Field(..) => {}
        khora_script::ast::decl::BehaviorMember::State(..) => {}
        khora_script::ast::decl::BehaviorMember::Method(..) => {}
        khora_script::ast::decl::BehaviorMember::Handler(..) => {}
        khora_script::ast::decl::BehaviorMember::Every(..) => {}
        khora_script::ast::decl::BehaviorMember::After(..) => {}
    }
}

fn every_decl_fields(x: &khora_script::ast::decl::EveryDecl) {
    let _ = (&x.interval, &x.body, &x.span);
}

fn field_fields(x: &khora_script::ast::decl::Field) {
    let _ = (&x.ty, &x.name, &x.default, &x.span);
}

fn function_decl_fields(x: &khora_script::ast::decl::FunctionDecl) {
    let _ = (
        &x.name,
        &x.name_span,
        &x.return_ty,
        &x.params,
        &x.body,
        &x.span,
    );
}

fn handler_decl_fields(x: &khora_script::ast::decl::HandlerDecl) {
    let _ = (&x.event, &x.event_span, &x.params, &x.body, &x.span);
}

fn import_fields(x: &khora_script::ast::decl::Import) {
    let _ = (&x.path, &x.alias, &x.span);
}

fn method_decl_fields(x: &khora_script::ast::decl::MethodDecl) {
    let _ = (
        &x.is_async,
        &x.return_ty,
        &x.name,
        &x.name_span,
        &x.params,
        &x.body,
        &x.span,
    );
}

fn operator_decl_fields(x: &khora_script::ast::decl::OperatorDecl) {
    let _ = (&x.op, &x.op_span, &x.return_ty, &x.params, &x.body, &x.span);
}

fn overloadable_op_variants(x: &khora_script::ast::decl::OverloadableOp) {
    match x {
        khora_script::ast::decl::OverloadableOp::Add => {}
        khora_script::ast::decl::OverloadableOp::Sub => {}
        khora_script::ast::decl::OverloadableOp::Mul => {}
        khora_script::ast::decl::OverloadableOp::Div => {}
        khora_script::ast::decl::OverloadableOp::Rem => {}
        khora_script::ast::decl::OverloadableOp::Eq => {}
        khora_script::ast::decl::OverloadableOp::NotEq => {}
        khora_script::ast::decl::OverloadableOp::Less => {}
        khora_script::ast::decl::OverloadableOp::LessEq => {}
        khora_script::ast::decl::OverloadableOp::Greater => {}
        khora_script::ast::decl::OverloadableOp::GreaterEq => {}
    }
}

fn param_fields(x: &khora_script::ast::decl::Param) {
    let _ = (&x.ty, &x.name, &x.span);
}

fn state_decl_fields(x: &khora_script::ast::decl::StateDecl) {
    let _ = (&x.name, &x.name_span, &x.params, &x.members, &x.span);
}

fn struct_decl_fields(x: &khora_script::ast::decl::StructDecl) {
    let _ = (&x.name, &x.name_span, &x.fields, &x.operators, &x.span);
}

fn binary_op_variants(x: &khora_script::ast::expr::BinaryOp) {
    match x {
        khora_script::ast::expr::BinaryOp::Add => {}
        khora_script::ast::expr::BinaryOp::Sub => {}
        khora_script::ast::expr::BinaryOp::Mul => {}
        khora_script::ast::expr::BinaryOp::Div => {}
        khora_script::ast::expr::BinaryOp::Rem => {}
        khora_script::ast::expr::BinaryOp::Eq => {}
        khora_script::ast::expr::BinaryOp::NotEq => {}
        khora_script::ast::expr::BinaryOp::Less => {}
        khora_script::ast::expr::BinaryOp::LessEq => {}
        khora_script::ast::expr::BinaryOp::Greater => {}
        khora_script::ast::expr::BinaryOp::GreaterEq => {}
        khora_script::ast::expr::BinaryOp::And => {}
        khora_script::ast::expr::BinaryOp::Or => {}
        khora_script::ast::expr::BinaryOp::Coalesce => {}
    }
}

fn expr_variants(x: &khora_script::ast::expr::Expr) {
    match x {
        khora_script::ast::expr::Expr::Int { .. } => {}
        khora_script::ast::expr::Expr::Float { .. } => {}
        khora_script::ast::expr::Expr::Str { .. } => {}
        khora_script::ast::expr::Expr::Duration { .. } => {}
        khora_script::ast::expr::Expr::Angle { .. } => {}
        khora_script::ast::expr::Expr::Bool { .. } => {}
        khora_script::ast::expr::Expr::Null(..) => {}
        khora_script::ast::expr::Expr::This(..) => {}
        khora_script::ast::expr::Expr::Ident { .. } => {}
        khora_script::ast::expr::Expr::ArrayLit { .. } => {}
        khora_script::ast::expr::Expr::Binary { .. } => {}
        khora_script::ast::expr::Expr::Unary { .. } => {}
        khora_script::ast::expr::Expr::Assign { .. } => {}
        khora_script::ast::expr::Expr::Call { .. } => {}
        khora_script::ast::expr::Expr::Field { .. } => {}
        khora_script::ast::expr::Expr::OptionalField { .. } => {}
        khora_script::ast::expr::Expr::Index { .. } => {}
        khora_script::ast::expr::Expr::Ternary { .. } => {}
        khora_script::ast::expr::Expr::New { .. } => {}
        khora_script::ast::expr::Expr::Binding { .. } => {}
        khora_script::ast::expr::Expr::Await { .. } => {}
        khora_script::ast::expr::Expr::Cast { .. } => {}
    }
}

fn unary_op_variants(x: &khora_script::ast::expr::UnaryOp) {
    match x {
        khora_script::ast::expr::UnaryOp::Neg => {}
        khora_script::ast::expr::UnaryOp::Not => {}
    }
}

fn block_fields(x: &khora_script::ast::stmt::Block) {
    let _ = (&x.statements, &x.span);
}

fn match_arm_fields(x: &khora_script::ast::stmt::MatchArm) {
    let _ = (&x.pattern, &x.body, &x.span);
}

fn pattern_variants(x: &khora_script::ast::stmt::Pattern) {
    match x {
        khora_script::ast::stmt::Pattern::Null(..) => {}
        khora_script::ast::stmt::Pattern::Binding { .. } => {}
        khora_script::ast::stmt::Pattern::Wildcard(..) => {}
    }
}

fn stmt_variants(x: &khora_script::ast::stmt::Stmt) {
    match x {
        khora_script::ast::stmt::Stmt::Let { .. } => {}
        khora_script::ast::stmt::Stmt::Expr(..) => {}
        khora_script::ast::stmt::Stmt::If { .. } => {}
        khora_script::ast::stmt::Stmt::While { .. } => {}
        khora_script::ast::stmt::Stmt::For { .. } => {}
        khora_script::ast::stmt::Stmt::Foreach { .. } => {}
        khora_script::ast::stmt::Stmt::Return { .. } => {}
        khora_script::ast::stmt::Stmt::Break(..) => {}
        khora_script::ast::stmt::Stmt::Continue(..) => {}
        khora_script::ast::stmt::Stmt::Become { .. } => {}
        khora_script::ast::stmt::Stmt::Match { .. } => {}
        khora_script::ast::stmt::Stmt::Block(..) => {}
    }
}

fn type_ref_variants(x: &khora_script::ast::types::TypeRef) {
    match x {
        khora_script::ast::types::TypeRef::Void => {}
        khora_script::ast::types::TypeRef::Named { .. } => {}
        khora_script::ast::types::TypeRef::Optional { .. } => {}
        khora_script::ast::types::TypeRef::Array { .. } => {}
        khora_script::ast::types::TypeRef::Generic { .. } => {}
    }
}

fn unrepresentable_variants(x: &khora_script::bridge::Unrepresentable) {
    match x {
        khora_script::bridge::Unrepresentable::Kind { .. } => {}
        khora_script::bridge::Unrepresentable::NoRoom => {}
    }
}

fn compiled_fields(x: &khora_script::bytecode::Compiled) {
    let _ = (&x.program, &x.diagnostics);
}

fn compiler_fields(x: &khora_script::bytecode::Compiler) {
    let _ = (
        &x.program,
        &x.diagnostics,
        &x.signatures,
        &x.returns,
        &x.natives,
        &x.behavior,
        &x.fields,
        &x.entries,
        &x.code,
        &x.registers,
    );
}

fn shape_variants(x: &khora_script::bytecode::Shape) {
    match x {
        khora_script::bytecode::Shape::Int => {}
        khora_script::bytecode::Shape::Float => {}
        khora_script::bytecode::Shape::Str => {}
        khora_script::bytecode::Shape::Engine(..) => {}
        khora_script::bytecode::Shape::Other => {}
    }
}

fn state_entry_fields(x: &khora_script::bytecode::StateEntry) {
    let _ = (&x.fields, &x.timers);
}

fn diagnostic_fields(x: &khora_script::diagnostics::Diagnostic) {
    let _ = (&x.severity, &x.message, &x.span, &x.note);
}

fn severity_variants(x: &khora_script::diagnostics::Severity) {
    match x {
        khora_script::diagnostics::Severity::Error => {}
        khora_script::diagnostics::Severity::Warning => {}
    }
}

fn span_fields(x: &khora_script::diagnostics::Span) {
    let _ = (&x.start, &x.end);
}

fn delivered_fields(x: &khora_script::dispatch::Delivered) {
    let _ = (&x.outcome, &x.spent, &x.suspended);
}

fn ticked_fields(x: &khora_script::dispatch::Ticked) {
    let _ = (&x.spent, &x.fault, &x.suspended);
}

fn suspended_timer_fields(x: &khora_script::dispatch::SuspendedTimer) {
    let _ = (&x.index, &x.machine, &x.rearm, &x.why);
}

fn not_delivered_variants(x: &khora_script::dispatch::NotDelivered) {
    match x {
        khora_script::dispatch::NotDelivered::NoSuchEntity(..) => {}
        khora_script::dispatch::NotDelivered::NoHandler { .. } => {}
        khora_script::dispatch::NotDelivered::WrongArity { .. } => {}
        khora_script::dispatch::NotDelivered::UnsupportedArgument { .. } => {}
    }
}

fn lexed_fields(x: &khora_script::lexer::Lexed) {
    let _ = (&x.tokens, &x.diagnostics);
}

fn keyword_variants(x: &khora_script::lexer::token::Keyword) {
    match x {
        khora_script::lexer::token::Keyword::Behavior => {}
        khora_script::lexer::token::Keyword::State => {}
        khora_script::lexer::token::Keyword::Become => {}
        khora_script::lexer::token::Keyword::On => {}
        khora_script::lexer::token::Keyword::Every => {}
        khora_script::lexer::token::Keyword::After => {}
        khora_script::lexer::token::Keyword::Async => {}
        khora_script::lexer::token::Keyword::Await => {}
        khora_script::lexer::token::Keyword::Import => {}
        khora_script::lexer::token::Keyword::As => {}
        khora_script::lexer::token::Keyword::Struct => {}
        khora_script::lexer::token::Keyword::Fn => {}
        khora_script::lexer::token::Keyword::Var => {}
        khora_script::lexer::token::Keyword::Void => {}
        khora_script::lexer::token::Keyword::Static => {}
        khora_script::lexer::token::Keyword::Operator => {}
        khora_script::lexer::token::Keyword::New => {}
        khora_script::lexer::token::Keyword::If => {}
        khora_script::lexer::token::Keyword::Else => {}
        khora_script::lexer::token::Keyword::While => {}
        khora_script::lexer::token::Keyword::For => {}
        khora_script::lexer::token::Keyword::Foreach => {}
        khora_script::lexer::token::Keyword::In => {}
        khora_script::lexer::token::Keyword::Match => {}
        khora_script::lexer::token::Keyword::Return => {}
        khora_script::lexer::token::Keyword::Break => {}
        khora_script::lexer::token::Keyword::Continue => {}
        khora_script::lexer::token::Keyword::True => {}
        khora_script::lexer::token::Keyword::False => {}
        khora_script::lexer::token::Keyword::Null => {}
        khora_script::lexer::token::Keyword::This => {}
    }
}

fn token_fields(x: &khora_script::lexer::token::Token) {
    let _ = (&x.kind, &x.span);
}

fn token_kind_variants(x: &khora_script::lexer::token::TokenKind) {
    match x {
        khora_script::lexer::token::TokenKind::Int(..) => {}
        khora_script::lexer::token::TokenKind::Float(..) => {}
        khora_script::lexer::token::TokenKind::Str(..) => {}
        khora_script::lexer::token::TokenKind::Duration(..) => {}
        khora_script::lexer::token::TokenKind::Angle(..) => {}
        khora_script::lexer::token::TokenKind::Ident(..) => {}
        khora_script::lexer::token::TokenKind::Keyword(..) => {}
        khora_script::lexer::token::TokenKind::LParen => {}
        khora_script::lexer::token::TokenKind::RParen => {}
        khora_script::lexer::token::TokenKind::LBrace => {}
        khora_script::lexer::token::TokenKind::RBrace => {}
        khora_script::lexer::token::TokenKind::LBracket => {}
        khora_script::lexer::token::TokenKind::RBracket => {}
        khora_script::lexer::token::TokenKind::Comma => {}
        khora_script::lexer::token::TokenKind::Semi => {}
        khora_script::lexer::token::TokenKind::Dot => {}
        khora_script::lexer::token::TokenKind::Colon => {}
        khora_script::lexer::token::TokenKind::Assign => {}
        khora_script::lexer::token::TokenKind::FatArrow => {}
        khora_script::lexer::token::TokenKind::Plus => {}
        khora_script::lexer::token::TokenKind::Minus => {}
        khora_script::lexer::token::TokenKind::Star => {}
        khora_script::lexer::token::TokenKind::Slash => {}
        khora_script::lexer::token::TokenKind::Percent => {}
        khora_script::lexer::token::TokenKind::PlusAssign => {}
        khora_script::lexer::token::TokenKind::MinusAssign => {}
        khora_script::lexer::token::TokenKind::StarAssign => {}
        khora_script::lexer::token::TokenKind::SlashAssign => {}
        khora_script::lexer::token::TokenKind::Eq => {}
        khora_script::lexer::token::TokenKind::NotEq => {}
        khora_script::lexer::token::TokenKind::Less => {}
        khora_script::lexer::token::TokenKind::LessEq => {}
        khora_script::lexer::token::TokenKind::Greater => {}
        khora_script::lexer::token::TokenKind::GreaterEq => {}
        khora_script::lexer::token::TokenKind::AndAnd => {}
        khora_script::lexer::token::TokenKind::OrOr => {}
        khora_script::lexer::token::TokenKind::Bang => {}
        khora_script::lexer::token::TokenKind::Question => {}
        khora_script::lexer::token::TokenKind::QuestionQuestion => {}
        khora_script::lexer::token::TokenKind::QuestionDot => {}
        khora_script::lexer::token::TokenKind::At => {}
        khora_script::lexer::token::TokenKind::Eof => {}
    }
}

fn lifecycle_fields(x: &khora_script::lifecycle::Lifecycle) {
    let _ = (&x.name, &x.params, &x.called);
}

fn resolved_fields(x: &khora_script::modules::Resolved) {
    let _ = (&x.modules, &x.diagnostics);
}

fn resolved_module_fields(x: &khora_script::modules::ResolvedModule) {
    let _ = (&x.path, &x.source, &x.module);
}

fn compile_outcome_fields(x: &khora_script::pipeline::CompileOutcome) {
    let _: &Option<khora_script::vm::Program> = &x.program;
    let _: &Vec<khora_script::diagnostics::Diagnostic> = &x.diagnostics;
}

fn path_error_variants(x: &khora_script::modules::path::PathError) {
    match x {
        khora_script::modules::path::PathError::Absolute => {}
        khora_script::modules::path::PathError::Escapes => {}
        khora_script::modules::path::PathError::Empty => {}
        khora_script::modules::path::PathError::WrongExtension => {}
    }
}

fn host_fields(x: &khora_script::native::Host) {
    let _ = (
        &x.natives,
        &x.commands,
        &x.arena,
        &x.fields,
        &x.input,
        &x.awaiting,
        &x.entity,
        &x.position,
        &x.outbox,
    );
}

fn native_context_fields(x: &khora_script::native::NativeContext<'static>) {
    let _ = (
        &x.commands,
        &x.arena,
        &x.entity,
        &x.input,
        &x.strings,
        &x.position,
        &x.events,
    );
}

fn native_error_fields(x: &khora_script::native::NativeError) {
    let _ = (&x.message,);
}

fn native_fn_fields(x: &khora_script::native::NativeFn) {
    let _ = (&x.name, &x.params, &x.result, &x.cost, &x.variadic, &x.call);
}

fn native_registration_fields(x: &khora_script::native::NativeRegistration) {
    let _ = (&x.0,);
}

fn native_ty_variants(x: &khora_script::native::ty::NativeTy) {
    match x {
        khora_script::native::ty::NativeTy::Void => {}
        khora_script::native::ty::NativeTy::Int => {}
        khora_script::native::ty::NativeTy::Float => {}
        khora_script::native::ty::NativeTy::Bool => {}
        khora_script::native::ty::NativeTy::Str => {}
        khora_script::native::ty::NativeTy::Duration => {}
        khora_script::native::ty::NativeTy::Angle => {}
        khora_script::native::ty::NativeTy::Entity => {}
        khora_script::native::ty::NativeTy::Engine(..) => {}
        khora_script::native::ty::NativeTy::Optional(..) => {}
        khora_script::native::ty::NativeTy::Array(..) => {}
    }
}

fn parsed_fields(x: &khora_script::parser::Parsed) {
    let _ = (&x.module, &x.diagnostics);
}

fn script_reload_fields(x: &khora_script::reload::ScriptReload) {
    let _ = (&x.module, &x.program);
}

fn checked_fields(x: &khora_script::types::Checked) {
    let _ = (&x.diagnostics,);
}

fn checker_fields(x: &khora_script::types::Checker) {
    let _ = (
        &x.structs,
        &x.functions,
        &x.natives,
        &x.behaviors,
        &x.diagnostics,
        &x.scopes,
    );
}

fn context_fields(x: &khora_script::types::Context) {
    let _ = (
        &x.allows_await,
        &x.member,
        &x.returns,
        &x.in_loop,
        &x.states,
        &x.owner,
    );
}

fn fn_info_fields(x: &khora_script::types::FnInfo) {
    let _ = (&x.params, &x.result, &x.variadic);
}

fn operator_info_fields(x: &khora_script::types::OperatorInfo) {
    let _ = (&x.op, &x.params, &x.result);
}

fn struct_info_fields(x: &khora_script::types::StructInfo) {
    let _ = (&x.fields, &x.operators);
}

fn binding_fields(x: &khora_script::types::scope::Binding) {
    let _ = (&x.ty, &x.span);
}

fn ty_variants(x: &khora_script::types::ty::Ty) {
    match x {
        khora_script::types::ty::Ty::Void => {}
        khora_script::types::ty::Ty::Int => {}
        khora_script::types::ty::Ty::Float => {}
        khora_script::types::ty::Ty::Bool => {}
        khora_script::types::ty::Ty::Str => {}
        khora_script::types::ty::Ty::Duration => {}
        khora_script::types::ty::Ty::Angle => {}
        khora_script::types::ty::Ty::Entity => {}
        khora_script::types::ty::Ty::Engine(..) => {}
        khora_script::types::ty::Ty::Optional(..) => {}
        khora_script::types::ty::Ty::Array(..) => {}
        khora_script::types::ty::Ty::Map(..) => {}
        khora_script::types::ty::Ty::Struct(..) => {}
        khora_script::types::ty::Ty::Behavior(..) => {}
        khora_script::types::ty::Ty::Error => {}
    }
}

fn fault_variants(x: &khora_script::vm::Fault) {
    match x {
        khora_script::vm::Fault::BadRegister { .. } => {}
        khora_script::vm::Fault::BadJump { .. } => {}
        khora_script::vm::Fault::BadFunction { .. } => {}
        khora_script::vm::Fault::TypeMismatch { .. } => {}
        khora_script::vm::Fault::DivideByZero => {}
        khora_script::vm::Fault::StackOverflow => {}
        khora_script::vm::Fault::UnknownNative { .. } => {}
        khora_script::vm::Fault::BadString => {}
        khora_script::vm::Fault::ArenaFull => {}
        khora_script::vm::Fault::NoSubject => {}
        khora_script::vm::Fault::NativeFailed { .. } => {}
    }
}

fn run_variants(x: &khora_script::vm::Run) {
    match x {
        khora_script::vm::Run::Completed => {}
        khora_script::vm::Run::Suspended(..) => {}
        khora_script::vm::Run::Faulted(..) => {}
    }
}

fn suspension_variants(x: &khora_script::vm::Suspension) {
    match x {
        khora_script::vm::Suspension::OutOfFuel => {}
        khora_script::vm::Suspension::Awaiting => {}
    }
}

fn instruction_variants(x: &khora_script::vm::instruction::Instruction) {
    match x {
        khora_script::vm::instruction::Instruction::LoadConst { .. } => {}
        khora_script::vm::instruction::Instruction::Move { .. } => {}
        khora_script::vm::instruction::Instruction::AddInt { .. } => {}
        khora_script::vm::instruction::Instruction::SubInt { .. } => {}
        khora_script::vm::instruction::Instruction::MulInt { .. } => {}
        khora_script::vm::instruction::Instruction::DivInt { .. } => {}
        khora_script::vm::instruction::Instruction::RemInt { .. } => {}
        khora_script::vm::instruction::Instruction::NegInt { .. } => {}
        khora_script::vm::instruction::Instruction::AddFloat { .. } => {}
        khora_script::vm::instruction::Instruction::SubFloat { .. } => {}
        khora_script::vm::instruction::Instruction::MulFloat { .. } => {}
        khora_script::vm::instruction::Instruction::DivFloat { .. } => {}
        khora_script::vm::instruction::Instruction::NegFloat { .. } => {}
        khora_script::vm::instruction::Instruction::Eq { .. } => {}
        khora_script::vm::instruction::Instruction::Less { .. } => {}
        khora_script::vm::instruction::Instruction::LessEq { .. } => {}
        khora_script::vm::instruction::Instruction::Not { .. } => {}
        khora_script::vm::instruction::Instruction::IsNull { .. } => {}
        khora_script::vm::instruction::Instruction::Jump { .. } => {}
        khora_script::vm::instruction::Instruction::JumpIfNot { .. } => {}
        khora_script::vm::instruction::Instruction::Call { .. } => {}
        khora_script::vm::instruction::Instruction::Return { .. } => {}
        khora_script::vm::instruction::Instruction::Become { .. } => {}
        khora_script::vm::instruction::Instruction::LoadField { .. } => {}
        khora_script::vm::instruction::Instruction::StoreField { .. } => {}
        khora_script::vm::instruction::Instruction::LoadStr { .. } => {}
        khora_script::vm::instruction::Instruction::Concat { .. } => {}
        khora_script::vm::instruction::Instruction::NativeCall { .. } => {}
        khora_script::vm::instruction::Instruction::Await { .. } => {}
        khora_script::vm::instruction::Instruction::LoadSelf { .. } => {}
        khora_script::vm::instruction::Instruction::Yield => {}
        khora_script::vm::instruction::Instruction::Halt => {}
    }
}

fn behavior_layout_fields(x: &khora_script::vm::program::BehaviorLayout) {
    let _ = (&x.name, &x.fields, &x.states, &x.timers);
}

fn function_fields(x: &khora_script::vm::program::Function) {
    let _ = (&x.name, &x.arity, &x.registers, &x.code);
}

fn program_fields(x: &khora_script::vm::program::Program) {
    let _ = (&x.functions, &x.behaviors, &x.strings);
}

fn state_layout_fields(x: &khora_script::vm::program::StateLayout) {
    let _ = (&x.name, &x.slots);
}

fn timer_kind_variants(x: &khora_script::vm::program::TimerKind) {
    match x {
        khora_script::vm::program::TimerKind::Every => {}
        khora_script::vm::program::TimerKind::After => {}
    }
}

fn timer_layout_fields(x: &khora_script::vm::program::TimerLayout) {
    let _ = (&x.kind, &x.seconds, &x.member, &x.state);
}

fn str_error_variants(x: &khora_script::vm::value::StrError) {
    match x {
        khora_script::vm::value::StrError::NotAString(..) => {}
        khora_script::vm::value::StrError::NotInProgram => {}
        khora_script::vm::value::StrError::Gone => {}
    }
}

fn str_ref_variants(x: &khora_script::vm::value::StrRef) {
    match x {
        khora_script::vm::value::StrRef::Const(..) => {}
        khora_script::vm::value::StrRef::Arena(..) => {}
    }
}

fn value_variants(x: &khora_script::vm::value::Value) {
    match x {
        khora_script::vm::value::Value::Unit => {}
        khora_script::vm::value::Value::Int(..) => {}
        khora_script::vm::value::Value::Float(..) => {}
        khora_script::vm::value::Value::Bool(..) => {}
        khora_script::vm::value::Value::Entity(..) => {}
        khora_script::vm::value::Value::Str(..) => {}
        khora_script::vm::value::Value::Vec2(..) => {}
        khora_script::vm::value::Value::Vec3(..) => {}
        khora_script::vm::value::Value::Vec4(..) => {}
        khora_script::vm::value::Value::Quat(..) => {}
        khora_script::vm::value::Value::Color(..) => {}
        khora_script::vm::value::Value::Null => {}
    }
}

// ---------------------------------------------------------------------------
// Trait items, named through a generic parameter. A generic body is
// type-checked where it is written, so these need no implementor and are
// never instantiated. A re-exported trait is checked for identity by
// forwarding a bound both ways.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod trait_items {
    fn source_loader_trait_items<T: khora_script::modules::loader::SourceLoader>() {
        let _ = <T as khora_script::modules::loader::SourceLoader>::load;
    }

    fn source_loader_trait_identity<T: khora_script::modules::SourceLoader>() {
        source_loader_trait_items::<T>();
    }

    fn source_loader_trait_identity_rev<T: khora_script::modules::loader::SourceLoader>() {
        source_loader_trait_identity::<T>();
    }

    fn source_loader_trait_identity_2<T: khora_script::SourceLoader>() {
        source_loader_trait_items::<T>();
    }

    fn source_loader_trait_identity_2_rev<T: khora_script::modules::loader::SourceLoader>() {
        source_loader_trait_identity_2::<T>();
    }

    fn script_type_trait_items<T: khora_script::native::convert::ScriptType>() {
        let _ = <T as khora_script::native::convert::ScriptType>::TY;
        let _ = <T as khora_script::native::convert::ScriptType>::from_value;
        let _ = <T as khora_script::native::convert::ScriptType>::to_value;
    }

    fn script_type_trait_identity<T: khora_script::native::ScriptType>() {
        script_type_trait_items::<T>();
    }

    fn script_type_trait_identity_rev<T: khora_script::native::convert::ScriptType>() {
        script_type_trait_identity::<T>();
    }

    fn script_type_trait_identity_2<T: khora_script::ScriptType>() {
        script_type_trait_items::<T>();
    }

    fn script_type_trait_identity_2_rev<T: khora_script::native::convert::ScriptType>() {
        script_type_trait_identity_2::<T>();
    }
}

#[test]
fn module_crate_root_paths_still_resolve() {
    #[allow(unused_imports)]
    use khora_script::ergon_fn as _; // proc_attribute `khora_macros::ergon_fn`
    #[allow(unused_imports)]
    use khora_script::inventory as _; // module `inventory`
}

#[test]
fn module_arena_paths_still_resolve() {
    let _ = type_name::<khora_script::arena::Arena>();
    let _ = type_name::<khora_script::Arena>();
    same_type(
        PhantomData::<khora_script::Arena>,
        PhantomData::<khora_script::arena::Arena>,
    );
    let _ = khora_script::arena::Arena::new;
    let _ = khora_script::arena::Arena::generation;
    let _ = khora_script::arena::Arena::len;
    let _ = khora_script::arena::Arena::is_empty;
    let _ = khora_script::arena::Arena::alloc;
    let _ = khora_script::arena::Arena::get;
    let _ = khora_script::arena::Arena::get_mut;
    let _ = khora_script::arena::Arena::reset;
    is_debug::<khora_script::arena::Arena>();
    is_clone::<khora_script::arena::Arena>();
    is_partial_eq::<khora_script::arena::Arena>();
    is_serialize::<khora_script::arena::Arena>();
    is_deserialize_owned::<khora_script::arena::Arena>();
    is_default::<khora_script::arena::Arena>();
    let _ = type_name::<khora_script::arena::ArenaError>();
    let _ = arena_error_variants as fn(&khora_script::arena::ArenaError);
    let _ = khora_script::arena::ArenaError::message;
    let _ = khora_script::arena::ArenaError::note;
    is_debug::<khora_script::arena::ArenaError>();
    is_clone::<khora_script::arena::ArenaError>();
    is_copy::<khora_script::arena::ArenaError>();
    is_partial_eq::<khora_script::arena::ArenaError>();
    is_eq::<khora_script::arena::ArenaError>();
    let _ = type_name::<khora_script::arena::ArenaRef>();
    let _ = type_name::<khora_script::ArenaRef>();
    same_type(
        PhantomData::<khora_script::ArenaRef>,
        PhantomData::<khora_script::arena::ArenaRef>,
    );
    let _ = khora_script::arena::ArenaRef::generation;
    is_debug::<khora_script::arena::ArenaRef>();
    is_clone::<khora_script::arena::ArenaRef>();
    is_copy::<khora_script::arena::ArenaRef>();
    is_partial_eq::<khora_script::arena::ArenaRef>();
    is_eq::<khora_script::arena::ArenaRef>();
    is_hash::<khora_script::arena::ArenaRef>();
    is_serialize::<khora_script::arena::ArenaRef>();
    is_deserialize_owned::<khora_script::arena::ArenaRef>();
    let _ = type_name::<khora_script::arena::Object>();
    let _ = type_name::<khora_script::Object>();
    same_type(
        PhantomData::<khora_script::Object>,
        PhantomData::<khora_script::arena::Object>,
    );
    let _ = object_variants as fn(&khora_script::arena::Object);
    let _ = khora_script::arena::Object::len;
    let _ = khora_script::arena::Object::is_empty;
    let _ = khora_script::arena::Object::type_name;
    is_debug::<khora_script::arena::Object>();
    is_clone::<khora_script::arena::Object>();
    is_partial_eq::<khora_script::arena::Object>();
    is_serialize::<khora_script::arena::Object>();
    is_deserialize_owned::<khora_script::arena::Object>();
    let _ = type_name::<khora_script::arena::persistent::Persisted>();
    let _ = type_name::<khora_script::arena::Persisted>();
    same_type(
        PhantomData::<khora_script::arena::Persisted>,
        PhantomData::<khora_script::arena::persistent::Persisted>,
    );
    let _ = persisted_variants as fn(&khora_script::arena::persistent::Persisted);
    is_debug::<khora_script::arena::persistent::Persisted>();
    is_clone::<khora_script::arena::persistent::Persisted>();
    is_partial_eq::<khora_script::arena::persistent::Persisted>();
    is_serialize::<khora_script::arena::persistent::Persisted>();
    is_deserialize_owned::<khora_script::arena::persistent::Persisted>();
    let _ = type_name::<khora_script::arena::persistent::PersistentStore>();
    let _ = type_name::<khora_script::arena::PersistentStore>();
    let _ = type_name::<khora_script::PersistentStore>();
    same_type(
        PhantomData::<khora_script::arena::PersistentStore>,
        PhantomData::<khora_script::arena::persistent::PersistentStore>,
    );
    same_type(
        PhantomData::<khora_script::PersistentStore>,
        PhantomData::<khora_script::arena::persistent::PersistentStore>,
    );
    let _ = khora_script::arena::persistent::PersistentStore::new;
    let _ = khora_script::arena::persistent::PersistentStore::with_slots;
    let _ = khora_script::arena::persistent::PersistentStore::len;
    let _ = khora_script::arena::persistent::PersistentStore::is_empty;
    let _ = khora_script::arena::persistent::PersistentStore::get;
    let _ = khora_script::arena::persistent::PersistentStore::set;
    let _ = khora_script::arena::persistent::PersistentStore::store_object;
    let _ = khora_script::arena::persistent::PersistentStore::truncate;
    is_debug::<khora_script::arena::persistent::PersistentStore>();
    is_clone::<khora_script::arena::persistent::PersistentStore>();
    is_default::<khora_script::arena::persistent::PersistentStore>();
    is_partial_eq::<khora_script::arena::persistent::PersistentStore>();
    is_serialize::<khora_script::arena::persistent::PersistentStore>();
    is_deserialize_owned::<khora_script::arena::persistent::PersistentStore>();
}

#[test]
fn module_ast_paths_still_resolve() {
    let _ = type_name::<khora_script::ast::Item>();
    let _ = type_name::<khora_script::Item>();
    same_type(
        PhantomData::<khora_script::Item>,
        PhantomData::<khora_script::ast::Item>,
    );
    let _ = item_variants as fn(&khora_script::ast::Item);
    let _ = khora_script::ast::Item::name;
    let _ = khora_script::ast::Item::name_span;
    is_debug::<khora_script::ast::Item>();
    is_clone::<khora_script::ast::Item>();
    is_partial_eq::<khora_script::ast::Item>();
    let _ = type_name::<khora_script::ast::Module>();
    let _ = type_name::<khora_script::Module>();
    same_type(
        PhantomData::<khora_script::Module>,
        PhantomData::<khora_script::ast::Module>,
    );
    let _ = module_fields as fn(&khora_script::ast::Module);
    is_debug::<khora_script::ast::Module>();
    is_clone::<khora_script::ast::Module>();
    is_partial_eq::<khora_script::ast::Module>();
    let _ = type_name::<khora_script::ast::decl::AfterDecl>();
    let _ = type_name::<khora_script::ast::AfterDecl>();
    same_type(
        PhantomData::<khora_script::ast::AfterDecl>,
        PhantomData::<khora_script::ast::decl::AfterDecl>,
    );
    let _ = after_decl_fields as fn(&khora_script::ast::decl::AfterDecl);
    is_debug::<khora_script::ast::decl::AfterDecl>();
    is_clone::<khora_script::ast::decl::AfterDecl>();
    is_partial_eq::<khora_script::ast::decl::AfterDecl>();
    let _ = type_name::<khora_script::ast::decl::Attribute>();
    let _ = type_name::<khora_script::ast::Attribute>();
    same_type(
        PhantomData::<khora_script::ast::Attribute>,
        PhantomData::<khora_script::ast::decl::Attribute>,
    );
    let _ = attribute_fields as fn(&khora_script::ast::decl::Attribute);
    is_debug::<khora_script::ast::decl::Attribute>();
    is_clone::<khora_script::ast::decl::Attribute>();
    is_partial_eq::<khora_script::ast::decl::Attribute>();
    let _ = type_name::<khora_script::ast::decl::BehaviorDecl>();
    let _ = type_name::<khora_script::ast::BehaviorDecl>();
    let _ = type_name::<khora_script::BehaviorDecl>();
    same_type(
        PhantomData::<khora_script::ast::BehaviorDecl>,
        PhantomData::<khora_script::ast::decl::BehaviorDecl>,
    );
    same_type(
        PhantomData::<khora_script::BehaviorDecl>,
        PhantomData::<khora_script::ast::decl::BehaviorDecl>,
    );
    let _ = behavior_decl_fields as fn(&khora_script::ast::decl::BehaviorDecl);
    is_debug::<khora_script::ast::decl::BehaviorDecl>();
    is_clone::<khora_script::ast::decl::BehaviorDecl>();
    is_partial_eq::<khora_script::ast::decl::BehaviorDecl>();
    let _ = type_name::<khora_script::ast::decl::BehaviorMember>();
    let _ = type_name::<khora_script::ast::BehaviorMember>();
    same_type(
        PhantomData::<khora_script::ast::BehaviorMember>,
        PhantomData::<khora_script::ast::decl::BehaviorMember>,
    );
    let _ = behavior_member_variants as fn(&khora_script::ast::decl::BehaviorMember);
    let _ = khora_script::ast::decl::BehaviorMember::span;
    is_debug::<khora_script::ast::decl::BehaviorMember>();
    is_clone::<khora_script::ast::decl::BehaviorMember>();
    is_partial_eq::<khora_script::ast::decl::BehaviorMember>();
    let _ = type_name::<khora_script::ast::decl::EveryDecl>();
    let _ = type_name::<khora_script::ast::EveryDecl>();
    same_type(
        PhantomData::<khora_script::ast::EveryDecl>,
        PhantomData::<khora_script::ast::decl::EveryDecl>,
    );
    let _ = every_decl_fields as fn(&khora_script::ast::decl::EveryDecl);
    is_debug::<khora_script::ast::decl::EveryDecl>();
    is_clone::<khora_script::ast::decl::EveryDecl>();
    is_partial_eq::<khora_script::ast::decl::EveryDecl>();
    let _ = type_name::<khora_script::ast::decl::Field>();
    let _ = type_name::<khora_script::ast::Field>();
    same_type(
        PhantomData::<khora_script::ast::Field>,
        PhantomData::<khora_script::ast::decl::Field>,
    );
    let _ = field_fields as fn(&khora_script::ast::decl::Field);
    is_debug::<khora_script::ast::decl::Field>();
    is_clone::<khora_script::ast::decl::Field>();
    is_partial_eq::<khora_script::ast::decl::Field>();
    let _ = type_name::<khora_script::ast::decl::FunctionDecl>();
    let _ = type_name::<khora_script::ast::FunctionDecl>();
    same_type(
        PhantomData::<khora_script::ast::FunctionDecl>,
        PhantomData::<khora_script::ast::decl::FunctionDecl>,
    );
    let _ = function_decl_fields as fn(&khora_script::ast::decl::FunctionDecl);
    is_debug::<khora_script::ast::decl::FunctionDecl>();
    is_clone::<khora_script::ast::decl::FunctionDecl>();
    is_partial_eq::<khora_script::ast::decl::FunctionDecl>();
    let _ = type_name::<khora_script::ast::decl::HandlerDecl>();
    let _ = type_name::<khora_script::ast::HandlerDecl>();
    same_type(
        PhantomData::<khora_script::ast::HandlerDecl>,
        PhantomData::<khora_script::ast::decl::HandlerDecl>,
    );
    let _ = handler_decl_fields as fn(&khora_script::ast::decl::HandlerDecl);
    is_debug::<khora_script::ast::decl::HandlerDecl>();
    is_clone::<khora_script::ast::decl::HandlerDecl>();
    is_partial_eq::<khora_script::ast::decl::HandlerDecl>();
    let _ = type_name::<khora_script::ast::decl::Import>();
    let _ = type_name::<khora_script::ast::Import>();
    same_type(
        PhantomData::<khora_script::ast::Import>,
        PhantomData::<khora_script::ast::decl::Import>,
    );
    let _ = import_fields as fn(&khora_script::ast::decl::Import);
    is_debug::<khora_script::ast::decl::Import>();
    is_clone::<khora_script::ast::decl::Import>();
    is_partial_eq::<khora_script::ast::decl::Import>();
    let _ = type_name::<khora_script::ast::decl::MethodDecl>();
    let _ = type_name::<khora_script::ast::MethodDecl>();
    same_type(
        PhantomData::<khora_script::ast::MethodDecl>,
        PhantomData::<khora_script::ast::decl::MethodDecl>,
    );
    let _ = method_decl_fields as fn(&khora_script::ast::decl::MethodDecl);
    is_debug::<khora_script::ast::decl::MethodDecl>();
    is_clone::<khora_script::ast::decl::MethodDecl>();
    is_partial_eq::<khora_script::ast::decl::MethodDecl>();
    let _ = type_name::<khora_script::ast::decl::OperatorDecl>();
    let _ = type_name::<khora_script::ast::OperatorDecl>();
    same_type(
        PhantomData::<khora_script::ast::OperatorDecl>,
        PhantomData::<khora_script::ast::decl::OperatorDecl>,
    );
    let _ = operator_decl_fields as fn(&khora_script::ast::decl::OperatorDecl);
    is_debug::<khora_script::ast::decl::OperatorDecl>();
    is_clone::<khora_script::ast::decl::OperatorDecl>();
    is_partial_eq::<khora_script::ast::decl::OperatorDecl>();
    let _ = type_name::<khora_script::ast::decl::OverloadableOp>();
    let _ = type_name::<khora_script::ast::OverloadableOp>();
    same_type(
        PhantomData::<khora_script::ast::OverloadableOp>,
        PhantomData::<khora_script::ast::decl::OverloadableOp>,
    );
    let _ = overloadable_op_variants as fn(&khora_script::ast::decl::OverloadableOp);
    is_debug::<khora_script::ast::decl::OverloadableOp>();
    is_clone::<khora_script::ast::decl::OverloadableOp>();
    is_copy::<khora_script::ast::decl::OverloadableOp>();
    is_partial_eq::<khora_script::ast::decl::OverloadableOp>();
    is_eq::<khora_script::ast::decl::OverloadableOp>();
    let _ = type_name::<khora_script::ast::decl::Param>();
    let _ = type_name::<khora_script::ast::Param>();
    same_type(
        PhantomData::<khora_script::ast::Param>,
        PhantomData::<khora_script::ast::decl::Param>,
    );
    let _ = param_fields as fn(&khora_script::ast::decl::Param);
    is_debug::<khora_script::ast::decl::Param>();
    is_clone::<khora_script::ast::decl::Param>();
    is_partial_eq::<khora_script::ast::decl::Param>();
    let _ = type_name::<khora_script::ast::decl::StateDecl>();
    let _ = type_name::<khora_script::ast::StateDecl>();
    same_type(
        PhantomData::<khora_script::ast::StateDecl>,
        PhantomData::<khora_script::ast::decl::StateDecl>,
    );
    let _ = state_decl_fields as fn(&khora_script::ast::decl::StateDecl);
    is_debug::<khora_script::ast::decl::StateDecl>();
    is_clone::<khora_script::ast::decl::StateDecl>();
    is_partial_eq::<khora_script::ast::decl::StateDecl>();
    let _ = type_name::<khora_script::ast::decl::StructDecl>();
    let _ = type_name::<khora_script::ast::StructDecl>();
    same_type(
        PhantomData::<khora_script::ast::StructDecl>,
        PhantomData::<khora_script::ast::decl::StructDecl>,
    );
    let _ = struct_decl_fields as fn(&khora_script::ast::decl::StructDecl);
    is_debug::<khora_script::ast::decl::StructDecl>();
    is_clone::<khora_script::ast::decl::StructDecl>();
    is_partial_eq::<khora_script::ast::decl::StructDecl>();
    let _ = type_name::<khora_script::ast::expr::BinaryOp>();
    let _ = type_name::<khora_script::ast::BinaryOp>();
    same_type(
        PhantomData::<khora_script::ast::BinaryOp>,
        PhantomData::<khora_script::ast::expr::BinaryOp>,
    );
    let _ = binary_op_variants as fn(&khora_script::ast::expr::BinaryOp);
    let _ = khora_script::ast::expr::BinaryOp::overloadable;
    is_debug::<khora_script::ast::expr::BinaryOp>();
    is_clone::<khora_script::ast::expr::BinaryOp>();
    is_copy::<khora_script::ast::expr::BinaryOp>();
    is_partial_eq::<khora_script::ast::expr::BinaryOp>();
    is_eq::<khora_script::ast::expr::BinaryOp>();
    let _ = type_name::<khora_script::ast::expr::Expr>();
    let _ = type_name::<khora_script::ast::Expr>();
    let _ = type_name::<khora_script::Expr>();
    same_type(
        PhantomData::<khora_script::ast::Expr>,
        PhantomData::<khora_script::ast::expr::Expr>,
    );
    same_type(
        PhantomData::<khora_script::Expr>,
        PhantomData::<khora_script::ast::expr::Expr>,
    );
    let _ = expr_variants as fn(&khora_script::ast::expr::Expr);
    let _ = khora_script::ast::expr::Expr::span;
    is_debug::<khora_script::ast::expr::Expr>();
    is_clone::<khora_script::ast::expr::Expr>();
    is_partial_eq::<khora_script::ast::expr::Expr>();
    let _ = type_name::<khora_script::ast::expr::UnaryOp>();
    let _ = type_name::<khora_script::ast::UnaryOp>();
    same_type(
        PhantomData::<khora_script::ast::UnaryOp>,
        PhantomData::<khora_script::ast::expr::UnaryOp>,
    );
    let _ = unary_op_variants as fn(&khora_script::ast::expr::UnaryOp);
    is_debug::<khora_script::ast::expr::UnaryOp>();
    is_clone::<khora_script::ast::expr::UnaryOp>();
    is_copy::<khora_script::ast::expr::UnaryOp>();
    is_partial_eq::<khora_script::ast::expr::UnaryOp>();
    is_eq::<khora_script::ast::expr::UnaryOp>();
    let _ = type_name::<khora_script::ast::stmt::Block>();
    let _ = type_name::<khora_script::ast::Block>();
    same_type(
        PhantomData::<khora_script::ast::Block>,
        PhantomData::<khora_script::ast::stmt::Block>,
    );
    let _ = block_fields as fn(&khora_script::ast::stmt::Block);
    is_debug::<khora_script::ast::stmt::Block>();
    is_clone::<khora_script::ast::stmt::Block>();
    is_partial_eq::<khora_script::ast::stmt::Block>();
    let _ = type_name::<khora_script::ast::stmt::MatchArm>();
    let _ = type_name::<khora_script::ast::MatchArm>();
    same_type(
        PhantomData::<khora_script::ast::MatchArm>,
        PhantomData::<khora_script::ast::stmt::MatchArm>,
    );
    let _ = match_arm_fields as fn(&khora_script::ast::stmt::MatchArm);
    is_debug::<khora_script::ast::stmt::MatchArm>();
    is_clone::<khora_script::ast::stmt::MatchArm>();
    is_partial_eq::<khora_script::ast::stmt::MatchArm>();
    let _ = type_name::<khora_script::ast::stmt::Pattern>();
    let _ = type_name::<khora_script::ast::Pattern>();
    same_type(
        PhantomData::<khora_script::ast::Pattern>,
        PhantomData::<khora_script::ast::stmt::Pattern>,
    );
    let _ = pattern_variants as fn(&khora_script::ast::stmt::Pattern);
    let _ = khora_script::ast::stmt::Pattern::span;
    is_debug::<khora_script::ast::stmt::Pattern>();
    is_clone::<khora_script::ast::stmt::Pattern>();
    is_partial_eq::<khora_script::ast::stmt::Pattern>();
    let _ = type_name::<khora_script::ast::stmt::Stmt>();
    let _ = type_name::<khora_script::ast::Stmt>();
    let _ = type_name::<khora_script::Stmt>();
    same_type(
        PhantomData::<khora_script::ast::Stmt>,
        PhantomData::<khora_script::ast::stmt::Stmt>,
    );
    same_type(
        PhantomData::<khora_script::Stmt>,
        PhantomData::<khora_script::ast::stmt::Stmt>,
    );
    let _ = stmt_variants as fn(&khora_script::ast::stmt::Stmt);
    let _ = khora_script::ast::stmt::Stmt::span;
    is_debug::<khora_script::ast::stmt::Stmt>();
    is_clone::<khora_script::ast::stmt::Stmt>();
    is_partial_eq::<khora_script::ast::stmt::Stmt>();
    let _ = type_name::<khora_script::ast::types::TypeRef>();
    let _ = type_name::<khora_script::ast::TypeRef>();
    let _ = type_name::<khora_script::TypeRef>();
    same_type(
        PhantomData::<khora_script::ast::TypeRef>,
        PhantomData::<khora_script::ast::types::TypeRef>,
    );
    same_type(
        PhantomData::<khora_script::TypeRef>,
        PhantomData::<khora_script::ast::types::TypeRef>,
    );
    let _ = type_ref_variants as fn(&khora_script::ast::types::TypeRef);
    let _ = khora_script::ast::types::TypeRef::span;
    let _ = khora_script::ast::types::TypeRef::is_optional;
    is_debug::<khora_script::ast::types::TypeRef>();
    is_clone::<khora_script::ast::types::TypeRef>();
    is_partial_eq::<khora_script::ast::types::TypeRef>();
}

#[test]
fn module_bridge_paths_still_resolve() {
    let _ = type_name::<khora_script::bridge::Unrepresentable>();
    let _ = unrepresentable_variants as fn(&khora_script::bridge::Unrepresentable);
    is_debug::<khora_script::bridge::Unrepresentable>();
    is_clone::<khora_script::bridge::Unrepresentable>();
    is_partial_eq::<khora_script::bridge::Unrepresentable>();
    is_eq::<khora_script::bridge::Unrepresentable>();
    is_display::<khora_script::bridge::Unrepresentable>();
    let _ = khora_script::bridge::from_persisted;
    let _ = khora_script::bridge::from_register;
    let _ = khora_script::bridge::to_persisted;
    let _ = khora_script::bridge::to_register;
}

#[test]
fn module_bytecode_paths_still_resolve() {
    let _ = type_name::<khora_script::bytecode::Compiled>();
    let _ = type_name::<khora_script::Compiled>();
    same_type(
        PhantomData::<khora_script::Compiled>,
        PhantomData::<khora_script::bytecode::Compiled>,
    );
    let _ = compiled_fields as fn(&khora_script::bytecode::Compiled);
    let _ = khora_script::bytecode::Compiled::has_errors;
    is_debug::<khora_script::bytecode::Compiled>();
    is_clone::<khora_script::bytecode::Compiled>();
    let _ = type_name::<khora_script::bytecode::Compiler>();
    let _ = compiler_fields as fn(&khora_script::bytecode::Compiler);
    let _ = khora_script::bytecode::Compiler::compile_expr;
    let _ = khora_script::bytecode::Compiler::arithmetic;
    let _ = khora_script::bytecode::Compiler::compile_block;
    let _ = khora_script::bytecode::Compiler::compile_stmt;
    let _ = khora_script::bytecode::Compiler::emit_state_entry;
    let _: fn(&mut khora_script::bytecode::Compiler, String, khora_script::diagnostics::Span) =
        khora_script::bytecode::Compiler::error;
    let _ = khora_script::bytecode::Compiler::emit;
    let _ = khora_script::bytecode::Compiler::here;
    let _ = khora_script::bytecode::Compiler::patch_to_here;
    let _ = khora_script::bytecode::Compiler::patch;
    let _ = khora_script::bytecode::Compiler::open_scope;
    let _ = khora_script::bytecode::Compiler::close_scope;
    let _ = khora_script::bytecode::Compiler::declare_local;
    let _ = khora_script::bytecode::Compiler::lookup_local;
    let _ = type_name::<khora_script::bytecode::Shape>();
    let _ = shape_variants as fn(&khora_script::bytecode::Shape);
    is_debug::<khora_script::bytecode::Shape>();
    is_clone::<khora_script::bytecode::Shape>();
    is_copy::<khora_script::bytecode::Shape>();
    is_partial_eq::<khora_script::bytecode::Shape>();
    is_eq::<khora_script::bytecode::Shape>();
    let _ = type_name::<khora_script::bytecode::StateEntry>();
    let _ = state_entry_fields as fn(&khora_script::bytecode::StateEntry);
    is_debug::<khora_script::bytecode::StateEntry>();
    is_clone::<khora_script::bytecode::StateEntry>();
    is_default::<khora_script::bytecode::StateEntry>();
    let _ = khora_script::bytecode::compile;
    let _ = khora_script::compile;
    same_item(&khora_script::compile, &khora_script::bytecode::compile);
    let _ = khora_script::bytecode::compile_with;
    let _ = khora_script::bytecode::init_name;
    let _ = type_name::<khora_script::bytecode::registers::Registers>();
    let _ = type_name::<khora_script::bytecode::Registers>();
    same_type(
        PhantomData::<khora_script::bytecode::Registers>,
        PhantomData::<khora_script::bytecode::registers::Registers>,
    );
    let _ = khora_script::bytecode::registers::Registers::new;
    let _ = khora_script::bytecode::registers::Registers::local;
    let _ = khora_script::bytecode::registers::Registers::temp;
    let _ = khora_script::bytecode::registers::Registers::mark;
    let _ = khora_script::bytecode::registers::Registers::release_to;
    let _ = khora_script::bytecode::registers::Registers::scope_mark;
    let _ = khora_script::bytecode::registers::Registers::close_scope;
    let _ = khora_script::bytecode::registers::Registers::frame_size;
    is_debug::<khora_script::bytecode::registers::Registers>();
    is_default::<khora_script::bytecode::registers::Registers>();
    let _ = khora_script::bytecode::shape_of;
    let _ = khora_script::bytecode::state_timer_name;
    let _ = khora_script::bytecode::timer_name;
}

#[test]
fn module_diagnostics_paths_still_resolve() {
    let _ = type_name::<khora_script::diagnostics::Diagnostic>();
    let _ = type_name::<khora_script::Diagnostic>();
    same_type(
        PhantomData::<khora_script::Diagnostic>,
        PhantomData::<khora_script::diagnostics::Diagnostic>,
    );
    let _ = diagnostic_fields as fn(&khora_script::diagnostics::Diagnostic);
    let _ = khora_script::diagnostics::Diagnostic::is_error;
    let _: fn(String, khora_script::diagnostics::Span) -> khora_script::diagnostics::Diagnostic =
        khora_script::diagnostics::Diagnostic::error;
    let _: fn(String, khora_script::diagnostics::Span) -> khora_script::diagnostics::Diagnostic =
        khora_script::diagnostics::Diagnostic::warning;
    let _: fn(
        khora_script::diagnostics::Diagnostic,
        String,
    ) -> khora_script::diagnostics::Diagnostic = khora_script::diagnostics::Diagnostic::with_note;
    let _ = khora_script::diagnostics::Diagnostic::render;
    is_debug::<khora_script::diagnostics::Diagnostic>();
    is_clone::<khora_script::diagnostics::Diagnostic>();
    is_partial_eq::<khora_script::diagnostics::Diagnostic>();
    is_eq::<khora_script::diagnostics::Diagnostic>();
    let _ = type_name::<khora_script::diagnostics::Severity>();
    let _ = type_name::<khora_script::Severity>();
    same_type(
        PhantomData::<khora_script::Severity>,
        PhantomData::<khora_script::diagnostics::Severity>,
    );
    let _ = severity_variants as fn(&khora_script::diagnostics::Severity);
    is_debug::<khora_script::diagnostics::Severity>();
    is_clone::<khora_script::diagnostics::Severity>();
    is_copy::<khora_script::diagnostics::Severity>();
    is_partial_eq::<khora_script::diagnostics::Severity>();
    is_eq::<khora_script::diagnostics::Severity>();
    is_display::<khora_script::diagnostics::Severity>();
    let _ = type_name::<khora_script::diagnostics::SourceFile>();
    let _ = type_name::<khora_script::SourceFile>();
    same_type(
        PhantomData::<khora_script::SourceFile>,
        PhantomData::<khora_script::diagnostics::SourceFile>,
    );
    let _: fn(String, String) -> khora_script::diagnostics::SourceFile =
        khora_script::diagnostics::SourceFile::new;
    let _ = khora_script::diagnostics::SourceFile::name;
    let _ = khora_script::diagnostics::SourceFile::text;
    let _ = khora_script::diagnostics::SourceFile::line_col;
    let _ = khora_script::diagnostics::SourceFile::line_text;
    is_debug::<khora_script::diagnostics::SourceFile>();
    is_clone::<khora_script::diagnostics::SourceFile>();
    let _ = type_name::<khora_script::diagnostics::Span>();
    let _ = type_name::<khora_script::Span>();
    same_type(
        PhantomData::<khora_script::Span>,
        PhantomData::<khora_script::diagnostics::Span>,
    );
    let _ = span_fields as fn(&khora_script::diagnostics::Span);
    let _ = khora_script::diagnostics::Span::new;
    let _ = khora_script::diagnostics::Span::empty;
    let _ = khora_script::diagnostics::Span::to;
    let _ = khora_script::diagnostics::Span::len;
    let _ = khora_script::diagnostics::Span::is_empty;
    is_debug::<khora_script::diagnostics::Span>();
    is_clone::<khora_script::diagnostics::Span>();
    is_copy::<khora_script::diagnostics::Span>();
    is_partial_eq::<khora_script::diagnostics::Span>();
    is_eq::<khora_script::diagnostics::Span>();
    is_partial_ord::<khora_script::diagnostics::Span>();
    is_ord::<khora_script::diagnostics::Span>();
    let _ = khora_script::diagnostics::has_errors;
}

#[test]
fn module_dispatch_paths_still_resolve() {
    let _ = type_name::<khora_script::dispatch::Delivered>();
    let _ = delivered_fields as fn(&khora_script::dispatch::Delivered);
    is_debug::<khora_script::dispatch::Delivered>();
    let _ = type_name::<khora_script::dispatch::NotDelivered>();
    let _ = type_name::<khora_script::NotDelivered>();
    same_type(
        PhantomData::<khora_script::NotDelivered>,
        PhantomData::<khora_script::dispatch::NotDelivered>,
    );
    let _ = not_delivered_variants as fn(&khora_script::dispatch::NotDelivered);
    is_debug::<khora_script::dispatch::NotDelivered>();
    is_clone::<khora_script::dispatch::NotDelivered>();
    is_partial_eq::<khora_script::dispatch::NotDelivered>();
    is_display::<khora_script::dispatch::NotDelivered>();
    let _ = khora_script::dispatch::current_state;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_script::vm::program::Program,
        &str,
        &khora_core::script::ScriptEvent,
        &mut khora_script::native::Host,
        u64,
        fn(khora_core::ecs::entity::EntityId) -> bool,
    ) -> Result<
        khora_script::dispatch::Delivered,
        khora_script::dispatch::NotDelivered,
    > = khora_script::dispatch::deliver;
    #[allow(clippy::type_complexity)]
    let _: fn(
        &khora_script::vm::program::Program,
        &str,
        &khora_core::script::ScriptEvent,
        &mut khora_script::native::Host,
        u64,
        fn(khora_core::ecs::entity::EntityId) -> bool,
    ) -> Result<
        khora_script::dispatch::Delivered,
        khora_script::dispatch::NotDelivered,
    > = khora_script::deliver;
    let _ = khora_script::dispatch::handler_name;
    let _ = khora_script::dispatch::handles;
    let _ = khora_script::handles;
    same_item(&khora_script::handles, &khora_script::dispatch::handles);
    let _ = khora_script::dispatch::initialise;
    let _ = khora_script::dispatch::invoke;
    let _ = khora_script::dispatch::resolve_member;
    let _ = khora_script::dispatch::state_handler_name;
    let _: fn(
        &khora_script::vm::program::Program,
        &str,
        &mut khora_script::native::Host,
        f32,
        u64,
    ) -> khora_script::dispatch::Ticked = khora_script::dispatch::tick_timers;
    let _ = type_name::<khora_script::dispatch::Ticked>();
    let _ = ticked_fields as fn(&khora_script::dispatch::Ticked);
    is_debug::<khora_script::dispatch::Ticked>();
    is_default::<khora_script::dispatch::Ticked>();
    let _ = type_name::<khora_script::dispatch::SuspendedTimer>();
    let _ = suspended_timer_fields as fn(&khora_script::dispatch::SuspendedTimer);
    is_debug::<khora_script::dispatch::SuspendedTimer>();
    let _: fn(
        &khora_script::vm::program::Program,
        &str,
        &mut khora_script::native::Host,
        usize,
        khora_script::vm::Value,
    ) = khora_script::dispatch::finish_timer;
}

#[test]
fn module_lexer_paths_still_resolve() {
    let _ = type_name::<khora_script::lexer::Lexed>();
    let _ = type_name::<khora_script::Lexed>();
    same_type(
        PhantomData::<khora_script::Lexed>,
        PhantomData::<khora_script::lexer::Lexed>,
    );
    let _ = lexed_fields as fn(&khora_script::lexer::Lexed);
    let _ = khora_script::lexer::Lexed::has_errors;
    is_debug::<khora_script::lexer::Lexed>();
    is_clone::<khora_script::lexer::Lexed>();
    let _ = khora_script::lexer::lex;
    let _ = khora_script::lex;
    same_item(&khora_script::lex, &khora_script::lexer::lex);
    let _ = type_name::<khora_script::lexer::token::Keyword>();
    let _ = type_name::<khora_script::lexer::Keyword>();
    let _ = type_name::<khora_script::Keyword>();
    same_type(
        PhantomData::<khora_script::lexer::Keyword>,
        PhantomData::<khora_script::lexer::token::Keyword>,
    );
    same_type(
        PhantomData::<khora_script::Keyword>,
        PhantomData::<khora_script::lexer::token::Keyword>,
    );
    let _ = keyword_variants as fn(&khora_script::lexer::token::Keyword);
    is_debug::<khora_script::lexer::token::Keyword>();
    is_clone::<khora_script::lexer::token::Keyword>();
    is_copy::<khora_script::lexer::token::Keyword>();
    is_partial_eq::<khora_script::lexer::token::Keyword>();
    is_eq::<khora_script::lexer::token::Keyword>();
    let _ = type_name::<khora_script::lexer::token::Token>();
    let _ = type_name::<khora_script::lexer::Token>();
    let _ = type_name::<khora_script::Token>();
    same_type(
        PhantomData::<khora_script::lexer::Token>,
        PhantomData::<khora_script::lexer::token::Token>,
    );
    same_type(
        PhantomData::<khora_script::Token>,
        PhantomData::<khora_script::lexer::token::Token>,
    );
    let _ = token_fields as fn(&khora_script::lexer::token::Token);
    is_debug::<khora_script::lexer::token::Token>();
    is_clone::<khora_script::lexer::token::Token>();
    is_partial_eq::<khora_script::lexer::token::Token>();
    let _ = type_name::<khora_script::lexer::token::TokenKind>();
    let _ = type_name::<khora_script::lexer::TokenKind>();
    let _ = type_name::<khora_script::TokenKind>();
    same_type(
        PhantomData::<khora_script::lexer::TokenKind>,
        PhantomData::<khora_script::lexer::token::TokenKind>,
    );
    same_type(
        PhantomData::<khora_script::TokenKind>,
        PhantomData::<khora_script::lexer::token::TokenKind>,
    );
    let _ = token_kind_variants as fn(&khora_script::lexer::token::TokenKind);
    is_debug::<khora_script::lexer::token::TokenKind>();
    is_clone::<khora_script::lexer::token::TokenKind>();
    is_partial_eq::<khora_script::lexer::token::TokenKind>();
}

#[test]
fn module_lifecycle_paths_still_resolve() {
    let _ = &khora_script::lifecycle::ALL;
    let _ = &khora_script::lifecycle::FIXED_UPDATE;
    let _ = type_name::<khora_script::lifecycle::Lifecycle>();
    let _ = lifecycle_fields as fn(&khora_script::lifecycle::Lifecycle);
    let _ = &khora_script::lifecycle::ON_DESPAWN;
    let _ = &khora_script::lifecycle::ON_LOAD;
    let _ = &khora_script::lifecycle::ON_SPAWN;
    let _ = &khora_script::lifecycle::UPDATE;
    let _ = khora_script::lifecycle::of;
}

#[test]
fn module_modules_paths_still_resolve() {
    let _ = type_name::<khora_script::modules::Resolved>();
    let _ = type_name::<khora_script::Resolved>();
    same_type(
        PhantomData::<khora_script::Resolved>,
        PhantomData::<khora_script::modules::Resolved>,
    );
    let _ = resolved_fields as fn(&khora_script::modules::Resolved);
    let _ = khora_script::modules::Resolved::has_errors;
    let _ = khora_script::modules::Resolved::get;
    is_debug::<khora_script::modules::Resolved>();
    is_clone::<khora_script::modules::Resolved>();
    is_default::<khora_script::modules::Resolved>();
    let _ = type_name::<khora_script::modules::ResolvedModule>();
    let _ = resolved_module_fields as fn(&khora_script::modules::ResolvedModule);
    is_debug::<khora_script::modules::ResolvedModule>();
    is_clone::<khora_script::modules::ResolvedModule>();
    let _ = type_name::<khora_script::modules::loader::MemoryLoader>();
    let _ = type_name::<khora_script::modules::MemoryLoader>();
    let _ = type_name::<khora_script::MemoryLoader>();
    same_type(
        PhantomData::<khora_script::modules::MemoryLoader>,
        PhantomData::<khora_script::modules::loader::MemoryLoader>,
    );
    same_type(
        PhantomData::<khora_script::MemoryLoader>,
        PhantomData::<khora_script::modules::loader::MemoryLoader>,
    );
    let _ = khora_script::modules::loader::MemoryLoader::new;
    let _: fn(
        khora_script::modules::loader::MemoryLoader,
        String,
        String,
    ) -> khora_script::modules::loader::MemoryLoader =
        khora_script::modules::loader::MemoryLoader::with;
    is_debug::<khora_script::modules::loader::MemoryLoader>();
    is_default::<khora_script::modules::loader::MemoryLoader>();
    is_clone::<khora_script::modules::loader::MemoryLoader>();
    is_source_loader::<khora_script::modules::loader::MemoryLoader>();
    // trait `khora_script::modules::loader::SourceLoader`: see `source_loader_trait_items`
    // trait `khora_script::modules::SourceLoader`: see `source_loader_trait_items`
    // trait `khora_script::SourceLoader`: see `source_loader_trait_items`
    let _ = type_name::<khora_script::modules::path::PathError>();
    let _ = type_name::<khora_script::modules::PathError>();
    same_type(
        PhantomData::<khora_script::modules::PathError>,
        PhantomData::<khora_script::modules::path::PathError>,
    );
    let _ = path_error_variants as fn(&khora_script::modules::path::PathError);
    let _ = khora_script::modules::path::PathError::message;
    let _ = khora_script::modules::path::PathError::note;
    is_debug::<khora_script::modules::path::PathError>();
    is_clone::<khora_script::modules::path::PathError>();
    is_copy::<khora_script::modules::path::PathError>();
    is_partial_eq::<khora_script::modules::path::PathError>();
    is_eq::<khora_script::modules::path::PathError>();
    let _ = khora_script::modules::path::normalise;
    let _ = khora_script::modules::normalise;
    same_item(
        &khora_script::modules::normalise,
        &khora_script::modules::path::normalise,
    );
    let _: &str = khora_script::modules::path::EXTENSION;
    let _: &str = khora_script::modules::EXTENSION;
    assert_eq!(
        khora_script::modules::EXTENSION,
        khora_script::modules::path::EXTENSION
    );
    let _: fn(&str) -> Vec<String> = khora_script::modules::imports_of;
    let _ = khora_script::modules::report_name_clashes;
    let _ = khora_script::modules::resolve;
    let _ = khora_script::resolve;
    same_item(&khora_script::resolve, &khora_script::modules::resolve);
}

#[test]
fn module_native_paths_still_resolve() {
    let _ = type_name::<khora_script::native::Host>();
    let _ = type_name::<khora_script::Host>();
    same_type(
        PhantomData::<khora_script::Host>,
        PhantomData::<khora_script::native::Host>,
    );
    let _ = host_fields as fn(&khora_script::native::Host);
    let _ = khora_script::native::Host::new;
    let _ = khora_script::native::Host::with_fields;
    let _ = khora_script::native::Host::bare;
    let _ = khora_script::native::Host::for_entity;
    let _ = khora_script::native::Host::end_frame;
    let _ = khora_script::native::Host::take_events;
    let _ = khora_script::native::Host::context;
    is_debug::<khora_script::native::Host>();
    is_default::<khora_script::native::Host>();
    let _ = type_name::<khora_script::native::NativeContext<'static>>();
    let _ = type_name::<khora_script::NativeContext<'static>>();
    same_type(
        PhantomData::<khora_script::NativeContext<'static>>,
        PhantomData::<khora_script::native::NativeContext<'static>>,
    );
    let _ = native_context_fields as fn(&khora_script::native::NativeContext<'static>);
    let _ = khora_script::native::NativeContext::string;
    let _ = type_name::<khora_script::native::NativeError>();
    let _ = type_name::<khora_script::NativeError>();
    same_type(
        PhantomData::<khora_script::NativeError>,
        PhantomData::<khora_script::native::NativeError>,
    );
    let _ = native_error_fields as fn(&khora_script::native::NativeError);
    let _: fn(String) -> khora_script::native::NativeError = khora_script::native::NativeError::new;
    is_debug::<khora_script::native::NativeError>();
    is_clone::<khora_script::native::NativeError>();
    is_partial_eq::<khora_script::native::NativeError>();
    is_eq::<khora_script::native::NativeError>();
    is_display::<khora_script::native::NativeError>();
    let _ = type_name::<khora_script::native::NativeFn>();
    let _ = type_name::<khora_script::NativeFn>();
    same_type(
        PhantomData::<khora_script::NativeFn>,
        PhantomData::<khora_script::native::NativeFn>,
    );
    let _ = native_fn_fields as fn(&khora_script::native::NativeFn);
    is_debug::<khora_script::native::NativeFn>();
    let _ = type_name::<khora_script::native::NativeRegistration>();
    let _ = native_registration_fields as fn(&khora_script::native::NativeRegistration);
    is_collect::<khora_script::native::NativeRegistration>();
    let _ = type_name::<khora_script::native::NativeRegistry>();
    let _ = type_name::<khora_script::NativeRegistry>();
    same_type(
        PhantomData::<khora_script::NativeRegistry>,
        PhantomData::<khora_script::native::NativeRegistry>,
    );
    let _ = khora_script::native::NativeRegistry::new;
    let _ = khora_script::native::NativeRegistry::with_builtins;
    let _ = khora_script::native::NativeRegistry::discovered;
    let _ = khora_script::native::NativeRegistry::register;
    let _ = khora_script::native::NativeRegistry::index_of;
    let _ = khora_script::native::NativeRegistry::get;
    let _ = khora_script::native::NativeRegistry::at;
    let _ = khora_script::native::NativeRegistry::is_empty;
    let _ = khora_script::native::NativeRegistry::len;
    let _ = khora_script::native::NativeRegistry::iter;
    is_debug::<khora_script::native::NativeRegistry>();
    is_default::<khora_script::native::NativeRegistry>();
    let _ = khora_script::native::accessor_name;
    let _ = &khora_script::native::builtins::ERGON_ABS;
    let _ = &khora_script::native::builtins::ERGON_CEIL;
    let _ = &khora_script::native::builtins::ERGON_CLAMP;
    let _ = &khora_script::native::builtins::ERGON_ERROR;
    let _ = &khora_script::native::builtins::ERGON_FLOOR;
    let _ = &khora_script::native::builtins::ERGON_LENGTH;
    let _ = &khora_script::native::builtins::ERGON_LERP;
    let _ = &khora_script::native::builtins::ERGON_LOG;
    let _ = &khora_script::native::builtins::ERGON_MAX;
    let _ = &khora_script::native::builtins::ERGON_MIN;
    let _ = &khora_script::native::builtins::ERGON_ROUND;
    let _ = &khora_script::native::builtins::ERGON_SIGN;
    let _ = &khora_script::native::builtins::ERGON_SQRT;
    let _ = &khora_script::native::builtins::ERGON_WARN;
    let _ = khora_script::native::builtins::builtins;
    let _ = khora_script::native::builtins;
    same_item(
        &khora_script::native::builtins,
        &khora_script::native::builtins::builtins,
    );
    // trait `khora_script::native::convert::ScriptType`: see `script_type_trait_items`
    // trait `khora_script::native::ScriptType`: see `script_type_trait_items`
    // trait `khora_script::ScriptType`: see `script_type_trait_items`
    let _ = &khora_script::native::events::ERGON_RAISE;
    let _ = &khora_script::native::ERGON_RAISE;
    assert!(std::ptr::eq(
        &khora_script::native::ERGON_RAISE,
        &khora_script::native::events::ERGON_RAISE
    ));
    let _ = &khora_script::native::input::ERGON_JUST_PRESSED;
    let _ = &khora_script::native::input::ERGON_PRESSED;
    let _ = type_name::<khora_script::native::ty::NativeTy>();
    let _ = type_name::<khora_script::native::NativeTy>();
    let _ = type_name::<khora_script::NativeTy>();
    same_type(
        PhantomData::<khora_script::native::NativeTy>,
        PhantomData::<khora_script::native::ty::NativeTy>,
    );
    same_type(
        PhantomData::<khora_script::NativeTy>,
        PhantomData::<khora_script::native::ty::NativeTy>,
    );
    let _ = native_ty_variants as fn(&khora_script::native::ty::NativeTy);
    let _ = khora_script::native::ty::NativeTy::to_ty;
    is_debug::<khora_script::native::ty::NativeTy>();
    is_clone::<khora_script::native::ty::NativeTy>();
    is_copy::<khora_script::native::ty::NativeTy>();
    is_partial_eq::<khora_script::native::ty::NativeTy>();
    is_eq::<khora_script::native::ty::NativeTy>();
    let _ = &khora_script::native::world::ERGON_DESPAWN;
    let _ = &khora_script::native::world::ERGON_DETACH;
    let _ = &khora_script::native::world::ERGON_POSITION;
    let _ = &khora_script::native::world::ERGON_SET_PARENT;
    let _ = &khora_script::native::world::ERGON_SET_POSITION;
    let _ = &khora_script::native::world::ERGON_SET_SCALE;
    let _ = &khora_script::native::world::ERGON_TRANSLATE;
}

#[test]
fn module_parser_paths_still_resolve() {
    let _ = type_name::<khora_script::parser::Parsed>();
    let _ = type_name::<khora_script::Parsed>();
    same_type(
        PhantomData::<khora_script::Parsed>,
        PhantomData::<khora_script::parser::Parsed>,
    );
    let _ = parsed_fields as fn(&khora_script::parser::Parsed);
    let _ = khora_script::parser::Parsed::has_errors;
    is_debug::<khora_script::parser::Parsed>();
    is_clone::<khora_script::parser::Parsed>();
    let _ = khora_script::parser::parse;
    let _ = khora_script::parse;
    same_item(&khora_script::parse, &khora_script::parser::parse);
}

#[test]
fn module_pipeline_paths_still_resolve() {
    let _ = type_name::<khora_script::pipeline::CompileOutcome>();
    let _ = type_name::<khora_script::CompileOutcome>();
    same_type(
        PhantomData::<khora_script::CompileOutcome>,
        PhantomData::<khora_script::pipeline::CompileOutcome>,
    );
    let _ = compile_outcome_fields as fn(&khora_script::pipeline::CompileOutcome);
    let _ = khora_script::pipeline::CompileOutcome::succeeded;
    is_debug::<khora_script::pipeline::CompileOutcome>();
    let _: fn(
        &dyn khora_script::modules::SourceLoader,
        &str,
    ) -> khora_script::pipeline::CompileOutcome = khora_script::pipeline::compile_module;
    let _ = khora_script::compile_module;
    same_item(
        &khora_script::compile_module,
        &khora_script::pipeline::compile_module,
    );
}

#[test]
fn module_reload_paths_still_resolve() {
    let _ = type_name::<khora_script::reload::ScriptReload>();
    let _ = script_reload_fields as fn(&khora_script::reload::ScriptReload);
    is_debug::<khora_script::reload::ScriptReload>();
    is_clone::<khora_script::reload::ScriptReload>();
    is_partial_eq::<khora_script::reload::ScriptReload>();
    is_supersedes::<khora_script::reload::ScriptReload>();
}

#[test]
fn module_types_paths_still_resolve() {
    let _ = type_name::<khora_script::types::Checked>();
    let _ = type_name::<khora_script::Checked>();
    same_type(
        PhantomData::<khora_script::Checked>,
        PhantomData::<khora_script::types::Checked>,
    );
    let _ = checked_fields as fn(&khora_script::types::Checked);
    let _ = khora_script::types::Checked::has_errors;
    is_debug::<khora_script::types::Checked>();
    is_clone::<khora_script::types::Checked>();
    let _ = type_name::<khora_script::types::Checker>();
    let _ = checker_fields as fn(&khora_script::types::Checker);
    let _ = khora_script::types::Checker::check_expr;
    let _ = khora_script::types::Checker::unify;
    let _ = khora_script::types::Checker::expect_bool;
    let _ = khora_script::types::Checker::check_block;
    let _ = khora_script::types::Checker::check_stmt;
    let _: fn(&mut khora_script::types::Checker, String, khora_script::diagnostics::Span) =
        khora_script::types::Checker::error;
    let _: fn(&mut khora_script::types::Checker, String, khora_script::diagnostics::Span, String) =
        khora_script::types::Checker::error_note;
    let _ = khora_script::types::Checker::resolve;
    let _ = khora_script::types::Checker::expect_assignable;
    let _ = type_name::<khora_script::types::Context>();
    let _ = context_fields as fn(&khora_script::types::Context);
    let _: fn(String, khora_script::types::ty::Ty) -> khora_script::types::Context =
        khora_script::types::Context::sync;
    is_debug::<khora_script::types::Context>();
    is_clone::<khora_script::types::Context>();
    let _ = type_name::<khora_script::types::FnInfo>();
    let _ = fn_info_fields as fn(&khora_script::types::FnInfo);
    is_debug::<khora_script::types::FnInfo>();
    is_clone::<khora_script::types::FnInfo>();
    let _ = type_name::<khora_script::types::OperatorInfo>();
    let _ = operator_info_fields as fn(&khora_script::types::OperatorInfo);
    is_debug::<khora_script::types::OperatorInfo>();
    is_clone::<khora_script::types::OperatorInfo>();
    let _ = type_name::<khora_script::types::StructInfo>();
    let _ = struct_info_fields as fn(&khora_script::types::StructInfo);
    is_debug::<khora_script::types::StructInfo>();
    is_clone::<khora_script::types::StructInfo>();
    let _ = khora_script::types::check;
    let _ = khora_script::check;
    same_item(&khora_script::check, &khora_script::types::check);
    let _ = khora_script::types::check_with;
    let _ = type_name::<khora_script::types::scope::Binding>();
    let _ = binding_fields as fn(&khora_script::types::scope::Binding);
    is_debug::<khora_script::types::scope::Binding>();
    is_clone::<khora_script::types::scope::Binding>();
    let _ = type_name::<khora_script::types::scope::Scopes>();
    let _ = type_name::<khora_script::types::Scopes>();
    same_type(
        PhantomData::<khora_script::types::Scopes>,
        PhantomData::<khora_script::types::scope::Scopes>,
    );
    let _ = khora_script::types::scope::Scopes::new;
    let _ = khora_script::types::scope::Scopes::push;
    let _ = khora_script::types::scope::Scopes::push_narrowed;
    let _ = khora_script::types::scope::Scopes::pop;
    let _ = khora_script::types::scope::Scopes::declare;
    let _ = khora_script::types::scope::Scopes::lookup;
    let _ = khora_script::types::scope::Scopes::type_of;
    is_debug::<khora_script::types::scope::Scopes>();
    is_default::<khora_script::types::scope::Scopes>();
    let _ = khora_script::types::ty::ENGINE_TYPES;
    let _ = type_name::<khora_script::types::ty::Ty>();
    let _ = type_name::<khora_script::types::Ty>();
    let _ = type_name::<khora_script::Ty>();
    same_type(
        PhantomData::<khora_script::types::Ty>,
        PhantomData::<khora_script::types::ty::Ty>,
    );
    same_type(
        PhantomData::<khora_script::Ty>,
        PhantomData::<khora_script::types::ty::Ty>,
    );
    let _ = ty_variants as fn(&khora_script::types::ty::Ty);
    let _ = khora_script::types::ty::Ty::name;
    let _ = khora_script::types::ty::Ty::accepts;
    let _ = khora_script::types::ty::Ty::is_optional;
    let _ = khora_script::types::ty::Ty::unwrapped;
    let _ = khora_script::types::ty::Ty::is_numeric;
    let _ = khora_script::types::ty::Ty::is_unit;
    is_debug::<khora_script::types::ty::Ty>();
    is_clone::<khora_script::types::ty::Ty>();
    is_partial_eq::<khora_script::types::ty::Ty>();
    is_eq::<khora_script::types::ty::Ty>();
    let _ = khora_script::types::ty::resolve;
}

#[test]
fn module_vm_paths_still_resolve() {
    let _ = type_name::<khora_script::vm::Fault>();
    let _ = fault_variants as fn(&khora_script::vm::Fault);
    is_debug::<khora_script::vm::Fault>();
    is_clone::<khora_script::vm::Fault>();
    is_partial_eq::<khora_script::vm::Fault>();
    is_eq::<khora_script::vm::Fault>();
    let _ = type_name::<khora_script::vm::Machine>();
    let _ = type_name::<khora_script::Machine>();
    same_type(
        PhantomData::<khora_script::Machine>,
        PhantomData::<khora_script::vm::Machine>,
    );
    let _ = khora_script::vm::Machine::new;
    let _ = khora_script::vm::Machine::register;
    let _ = khora_script::vm::Machine::result;
    let _ = khora_script::vm::Machine::is_finished;
    let _ = khora_script::vm::Machine::program_counter;
    let _ = khora_script::vm::Machine::depth;
    let _ = khora_script::vm::Machine::run;
    let _ = khora_script::vm::Machine::run_counting;
    let _ = khora_script::vm::Machine::resolve_str;
    let _ = khora_script::vm::Machine::freeze;
    let _ = khora_script::vm::Machine::thaw;
    is_debug::<khora_script::vm::Machine>();
    is_clone::<khora_script::vm::Machine>();
    is_partial_eq::<khora_script::vm::Machine>();
    is_serialize::<khora_script::vm::Machine>();
    is_deserialize_owned::<khora_script::vm::Machine>();
    let _ = type_name::<khora_script::vm::Run>();
    let _ = type_name::<khora_script::Run>();
    same_type(
        PhantomData::<khora_script::Run>,
        PhantomData::<khora_script::vm::Run>,
    );
    let _ = run_variants as fn(&khora_script::vm::Run);
    is_debug::<khora_script::vm::Run>();
    is_clone::<khora_script::vm::Run>();
    is_partial_eq::<khora_script::vm::Run>();
    is_eq::<khora_script::vm::Run>();
    let _ = type_name::<khora_script::vm::Suspension>();
    let _ = type_name::<khora_script::Suspension>();
    same_type(
        PhantomData::<khora_script::Suspension>,
        PhantomData::<khora_script::vm::Suspension>,
    );
    let _ = suspension_variants as fn(&khora_script::vm::Suspension);
    is_debug::<khora_script::vm::Suspension>();
    is_clone::<khora_script::vm::Suspension>();
    is_copy::<khora_script::vm::Suspension>();
    is_partial_eq::<khora_script::vm::Suspension>();
    is_eq::<khora_script::vm::Suspension>();
    let _ = type_name::<khora_script::vm::instruction::Instruction>();
    let _ = type_name::<khora_script::vm::Instruction>();
    let _ = type_name::<khora_script::Instruction>();
    same_type(
        PhantomData::<khora_script::vm::Instruction>,
        PhantomData::<khora_script::vm::instruction::Instruction>,
    );
    same_type(
        PhantomData::<khora_script::Instruction>,
        PhantomData::<khora_script::vm::instruction::Instruction>,
    );
    let _ = instruction_variants as fn(&khora_script::vm::instruction::Instruction);
    let _ = khora_script::vm::instruction::Instruction::cost;
    is_debug::<khora_script::vm::instruction::Instruction>();
    is_clone::<khora_script::vm::instruction::Instruction>();
    is_partial_eq::<khora_script::vm::instruction::Instruction>();
    is_serialize::<khora_script::vm::instruction::Instruction>();
    is_deserialize_owned::<khora_script::vm::instruction::Instruction>();
    let _ = type_name::<khora_script::vm::instruction::Reg>();
    let _ = type_name::<khora_script::vm::Reg>();
    same_type(
        PhantomData::<khora_script::vm::Reg>,
        PhantomData::<khora_script::vm::instruction::Reg>,
    );
    let _ = type_name::<khora_script::vm::program::BehaviorLayout>();
    let _ = type_name::<khora_script::vm::BehaviorLayout>();
    same_type(
        PhantomData::<khora_script::vm::BehaviorLayout>,
        PhantomData::<khora_script::vm::program::BehaviorLayout>,
    );
    let _ = behavior_layout_fields as fn(&khora_script::vm::program::BehaviorLayout);
    let _ = khora_script::vm::program::BehaviorLayout::slot_of;
    let _ = khora_script::vm::program::BehaviorLayout::state_slot;
    let _ = khora_script::vm::program::BehaviorLayout::state_data_slot;
    let _ = khora_script::vm::program::BehaviorLayout::timer_slot;
    let _ = khora_script::vm::program::BehaviorLayout::slot_count;
    let _ = khora_script::vm::program::BehaviorLayout::state_index;
    let _ = khora_script::vm::program::BehaviorLayout::state_at;
    is_debug::<khora_script::vm::program::BehaviorLayout>();
    is_clone::<khora_script::vm::program::BehaviorLayout>();
    is_partial_eq::<khora_script::vm::program::BehaviorLayout>();
    is_default::<khora_script::vm::program::BehaviorLayout>();
    is_serialize::<khora_script::vm::program::BehaviorLayout>();
    is_deserialize_owned::<khora_script::vm::program::BehaviorLayout>();
    let _ = type_name::<khora_script::vm::program::Function>();
    let _ = type_name::<khora_script::vm::Function>();
    let _ = type_name::<khora_script::Function>();
    same_type(
        PhantomData::<khora_script::vm::Function>,
        PhantomData::<khora_script::vm::program::Function>,
    );
    same_type(
        PhantomData::<khora_script::Function>,
        PhantomData::<khora_script::vm::program::Function>,
    );
    let _ = function_fields as fn(&khora_script::vm::program::Function);
    is_debug::<khora_script::vm::program::Function>();
    is_clone::<khora_script::vm::program::Function>();
    is_partial_eq::<khora_script::vm::program::Function>();
    is_serialize::<khora_script::vm::program::Function>();
    is_deserialize_owned::<khora_script::vm::program::Function>();
    let _ = type_name::<khora_script::vm::program::Program>();
    let _ = type_name::<khora_script::vm::Program>();
    let _ = type_name::<khora_script::Program>();
    same_type(
        PhantomData::<khora_script::vm::Program>,
        PhantomData::<khora_script::vm::program::Program>,
    );
    same_type(
        PhantomData::<khora_script::Program>,
        PhantomData::<khora_script::vm::program::Program>,
    );
    let _ = program_fields as fn(&khora_script::vm::program::Program);
    let _ = khora_script::vm::program::Program::index_of;
    let _ = khora_script::vm::program::Program::string;
    let _ = khora_script::vm::program::Program::layout;
    let _ = khora_script::vm::program::Program::function;
    let _ = khora_script::vm::program::Program::fingerprint;
    is_debug::<khora_script::vm::program::Program>();
    is_clone::<khora_script::vm::program::Program>();
    is_partial_eq::<khora_script::vm::program::Program>();
    is_default::<khora_script::vm::program::Program>();
    is_serialize::<khora_script::vm::program::Program>();
    is_deserialize_owned::<khora_script::vm::program::Program>();
    let _ = type_name::<khora_script::vm::program::StateLayout>();
    let _ = type_name::<khora_script::vm::StateLayout>();
    same_type(
        PhantomData::<khora_script::vm::StateLayout>,
        PhantomData::<khora_script::vm::program::StateLayout>,
    );
    let _ = state_layout_fields as fn(&khora_script::vm::program::StateLayout);
    is_debug::<khora_script::vm::program::StateLayout>();
    is_clone::<khora_script::vm::program::StateLayout>();
    is_partial_eq::<khora_script::vm::program::StateLayout>();
    is_eq::<khora_script::vm::program::StateLayout>();
    is_default::<khora_script::vm::program::StateLayout>();
    is_serialize::<khora_script::vm::program::StateLayout>();
    is_deserialize_owned::<khora_script::vm::program::StateLayout>();
    let _ = type_name::<khora_script::vm::program::TimerKind>();
    let _ = type_name::<khora_script::vm::TimerKind>();
    same_type(
        PhantomData::<khora_script::vm::TimerKind>,
        PhantomData::<khora_script::vm::program::TimerKind>,
    );
    let _ = timer_kind_variants as fn(&khora_script::vm::program::TimerKind);
    is_debug::<khora_script::vm::program::TimerKind>();
    is_clone::<khora_script::vm::program::TimerKind>();
    is_copy::<khora_script::vm::program::TimerKind>();
    is_partial_eq::<khora_script::vm::program::TimerKind>();
    is_eq::<khora_script::vm::program::TimerKind>();
    is_serialize::<khora_script::vm::program::TimerKind>();
    is_deserialize_owned::<khora_script::vm::program::TimerKind>();
    let _ = type_name::<khora_script::vm::program::TimerLayout>();
    let _ = type_name::<khora_script::vm::TimerLayout>();
    same_type(
        PhantomData::<khora_script::vm::TimerLayout>,
        PhantomData::<khora_script::vm::program::TimerLayout>,
    );
    let _ = timer_layout_fields as fn(&khora_script::vm::program::TimerLayout);
    is_debug::<khora_script::vm::program::TimerLayout>();
    is_clone::<khora_script::vm::program::TimerLayout>();
    is_partial_eq::<khora_script::vm::program::TimerLayout>();
    is_serialize::<khora_script::vm::program::TimerLayout>();
    is_deserialize_owned::<khora_script::vm::program::TimerLayout>();
    let _ = type_name::<khora_script::vm::value::StrError>();
    let _ = type_name::<khora_script::vm::StrError>();
    same_type(
        PhantomData::<khora_script::vm::StrError>,
        PhantomData::<khora_script::vm::value::StrError>,
    );
    let _ = str_error_variants as fn(&khora_script::vm::value::StrError);
    is_debug::<khora_script::vm::value::StrError>();
    is_clone::<khora_script::vm::value::StrError>();
    is_copy::<khora_script::vm::value::StrError>();
    is_partial_eq::<khora_script::vm::value::StrError>();
    is_eq::<khora_script::vm::value::StrError>();
    let _ = type_name::<khora_script::vm::value::StrRef>();
    let _ = type_name::<khora_script::vm::StrRef>();
    same_type(
        PhantomData::<khora_script::vm::StrRef>,
        PhantomData::<khora_script::vm::value::StrRef>,
    );
    let _ = str_ref_variants as fn(&khora_script::vm::value::StrRef);
    is_debug::<khora_script::vm::value::StrRef>();
    is_clone::<khora_script::vm::value::StrRef>();
    is_copy::<khora_script::vm::value::StrRef>();
    is_partial_eq::<khora_script::vm::value::StrRef>();
    is_eq::<khora_script::vm::value::StrRef>();
    is_hash::<khora_script::vm::value::StrRef>();
    is_serialize::<khora_script::vm::value::StrRef>();
    is_deserialize_owned::<khora_script::vm::value::StrRef>();
    let _ = type_name::<khora_script::vm::value::Value>();
    let _ = type_name::<khora_script::vm::Value>();
    let _ = type_name::<khora_script::Value>();
    same_type(
        PhantomData::<khora_script::vm::Value>,
        PhantomData::<khora_script::vm::value::Value>,
    );
    same_type(
        PhantomData::<khora_script::Value>,
        PhantomData::<khora_script::vm::value::Value>,
    );
    let _ = value_variants as fn(&khora_script::vm::value::Value);
    let _ = khora_script::vm::value::Value::as_int;
    let _ = khora_script::vm::value::Value::as_float;
    let _ = khora_script::vm::value::Value::as_bool;
    let _ = khora_script::vm::value::Value::as_entity;
    let _ = khora_script::vm::value::Value::as_vec3;
    let _ = khora_script::vm::value::Value::as_str_ref;
    let _ = khora_script::vm::value::Value::is_null;
    let _ = khora_script::vm::value::Value::type_name;
    let _ = khora_script::vm::value::Value::freeze;
    let _ = khora_script::vm::value::Value::thaw;
    is_debug::<khora_script::vm::value::Value>();
    is_clone::<khora_script::vm::value::Value>();
    is_copy::<khora_script::vm::value::Value>();
    is_partial_eq::<khora_script::vm::value::Value>();
    is_serialize::<khora_script::vm::value::Value>();
    is_deserialize_owned::<khora_script::vm::value::Value>();
    let _ = khora_script::vm::value::resolve_str;
    let _ = khora_script::vm::resolve_str;
    same_item(
        &khora_script::vm::resolve_str,
        &khora_script::vm::value::resolve_str,
    );
}

// ---------------------------------------------------------------------------
// The paths `#[ergon_fn]` writes into the crate that uses it
// (`khora-macros/src/ergon_fn.rs`), spelled with the leading `::` the
// expansion uses. They resolve in the *user's* crate, so a move inside
// khora-script that drops one breaks every crate exposing a native; the
// `ergon_fn` module next to this one expands the attribute for real.
// ---------------------------------------------------------------------------

fn ergon_fn_script_type_items<T: ::khora_script::native::ScriptType>() {
    let _ = <T as ::khora_script::native::ScriptType>::TY;
    let _ = <T as ::khora_script::native::ScriptType>::from_value;
    let _ = <T as ::khora_script::native::ScriptType>::to_value;
}

/// The struct literal the expansion writes, field for field: a field added
/// without a default, or one renamed, breaks every `#[ergon_fn]`.
fn ergon_fn_native_fn_literal() -> ::khora_script::native::NativeFn {
    ::khora_script::native::NativeFn {
        name: "GuardLiteral",
        params: &[<f32 as ::khora_script::native::ScriptType>::TY],
        result: <f32 as ::khora_script::native::ScriptType>::TY,
        cost: 1,
        variadic: false,
        call: |context, args| {
            let arg0 = <f32 as ::khora_script::native::ScriptType>::from_value(
                args.first()
                    .copied()
                    .unwrap_or(::khora_script::vm::Value::Unit),
                context,
            )?;
            ::khora_script::native::ScriptType::to_value(arg0, context)
        },
    }
}

#[test]
fn paths_emitted_by_ergon_fn_still_resolve() {
    ergon_fn_script_type_items::<f32>();
    let _ = ergon_fn_native_fn_literal as fn() -> ::khora_script::native::NativeFn;
    let _ = ::khora_script::native::NativeRegistration;
    let _ = ::khora_script::vm::Value::Unit;
    // `ergon_fn` itself, re-exported from `khora_macros` at the crate root.
    #[allow(unused_imports)]
    use ::khora_script::ergon_fn as _;
    // `::khora_script::inventory::submit!` is expanded by the `ergon_fn`
    // module; the collector it submits to must be the one `native` iterates.
    fn is_collected<T: ::khora_script::inventory::Collect>() {}
    is_collected::<::khora_script::native::NativeRegistration>();
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded, macro bodies included).
// The trailing comment names the users. Items, modules and enum variants
// are imported; associated items are named in the test below.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod paths_used_by_other_crates {
    use khora_script::arena::Object::Str as _; // khora-lanes
    use khora_script::arena::Persisted as _; // khora-lanes
    use khora_script::arena::Persisted::Owned as _; // khora-lanes
    use khora_script::arena::PersistentStore as _; // khora-lanes
    use khora_script::bridge::from_persisted as _; // khora-lanes
    use khora_script::bridge::to_persisted as _; // khora-lanes
    use khora_script::check as _; // khora-agents, khora-lanes
    use khora_script::compile as _; // khora-agents, khora-lanes
    use khora_script::compile_module as _; // hub, khora-agents, khora-io
    use khora_script::dispatch::deliver as _; // khora-lanes
    use khora_script::dispatch::initialise as _; // khora-lanes
    use khora_script::dispatch::invoke as _; // khora-lanes
    use khora_script::dispatch::tick_timers as _; // khora-lanes
    use khora_script::dispatch::NotDelivered as _; // khora-lanes
    use khora_script::inventory::submit as _; // khora-macros (macro)
    use khora_script::lex as _; // khora-agents, khora-io, khora-lanes
    use khora_script::lifecycle as _; // khora-lanes
    use khora_script::modules::imports_of as _; // khora-io
    use khora_script::modules::SourceLoader as _; // khora-io
    use khora_script::modules::EXTENSION as _; // khora-io
    use khora_script::native::Host as _; // khora-lanes
    use khora_script::native::NativeFn as _; // khora-macros
    use khora_script::native::NativeRegistration as _; // khora-macros
    use khora_script::native::ScriptType as _; // khora-macros
    use khora_script::parse as _; // khora-agents, khora-lanes
    use khora_script::reload::ScriptReload as _; // khora-agents, khora-io, khora-lanes
    use khora_script::vm::BehaviorLayout as _; // khora-lanes
    use khora_script::vm::Machine as _; // khora-lanes
    use khora_script::vm::Program as _; // khora-agents, khora-lanes
    use khora_script::vm::Run as _; // khora-lanes
    use khora_script::vm::TimerKind as _; // khora-lanes
    use khora_script::vm::TimerLayout as _; // khora-lanes
    use khora_script::vm::Value as _; // khora-lanes
    use khora_script::vm::Value::Unit as _; // khora-macros
    use khora_script::CompileOutcome as _; // khora-io
    use khora_script::Diagnostic as _; // khora-io
    use khora_script::MemoryLoader as _; // hub, khora-io
    use khora_script::Program as _; // khora-lanes
    use khora_script::TokenKind as _; // khora-io
}

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_script::arena::PersistentStore::new; // khora-lanes
    let _ = <f32 as khora_script::native::ScriptType>::to_value; // khora-macros
}
