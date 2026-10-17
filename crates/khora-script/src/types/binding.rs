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

//! Named arguments and parameter defaults: which value each parameter of a
//! call receives.
//!
//! Positional arguments fill the parameters in order, then each named one
//! fills its own; what is left takes its default. The arguments are checked —
//! and later evaluated — in the order written, whatever order the parameters
//! are declared in. A call that names nothing and leaves nothing out binds as
//! it always did, and records nothing.

use crate::ast::{Expr, Label, Param};
use crate::diagnostics::Span;

use super::structs::is_data_constant;
use super::{Checker, Context, Ty};

/// Where one parameter's value comes from, at a call that names an argument
/// or leaves one out.
#[derive(Debug, Clone)]
pub enum Bound {
    /// The argument written at this index, in source order.
    Written(usize),
    /// The parameter's declared default, a constant — boxed, so the place
    /// it was checked at is the place it is compiled from.
    Default(Box<Expr>),
}

/// What a callee's parameters are, as a call is bound against them.
pub(crate) struct Callee<'a> {
    /// The callee, as messages name it.
    pub name: &'a str,
    /// Parameter types, in order.
    pub params: &'a [Ty],
    /// Parameter names, in order — empty for a callee that has none.
    pub names: &'a [String],
    /// Each parameter's default, where it has one.
    pub defaults: &'a [Option<Expr>],
}

impl Callee<'_> {
    /// How many parameters a call must give: those before the first default.
    fn required(&self) -> usize {
        self.defaults
            .iter()
            .position(Option::is_some)
            .unwrap_or(self.params.len())
    }

    /// Whether a call written `args` / `labels` binds as it always did: every
    /// parameter given, in order, none by name.
    pub fn plain(&self, args: &[Expr], labels: &[Option<Label>]) -> bool {
        labels.is_empty() && args.len() == self.params.len()
    }

    /// "2 arguments", "1 to 3 arguments".
    pub fn arity(&self) -> String {
        let (required, all) = (self.required(), self.params.len());
        if required == all {
            format!("{all} argument{}", if all == 1 { "" } else { "s" })
        } else {
            format!("{required} to {all} arguments")
        }
    }
}

/// A callable's signature.
#[derive(Debug, Clone)]
pub struct FnInfo {
    /// Parameter types.
    ///
    /// The exact list, unless [`variadic`](Self::variadic) — then the minimum.
    pub params: Vec<Ty>,
    /// Return type.
    pub result: Ty,
    /// Whether extra arguments are allowed.
    ///
    /// Only an engine function can be: a script's own always declares exactly
    /// what it takes. See [`NativeFn::variadic`](crate::native::NativeFn::variadic)
    /// for why one would be.
    pub variadic: bool,
    /// Parameter names, in order — what a named argument names.
    pub names: Vec<String>,
    /// Each parameter's default, what a call that leaves it out passes.
    pub defaults: Vec<Option<Expr>>,
}

/// A call as its arguments are checked: what is written between its
/// parentheses, and where it stands.
pub(crate) struct CallSite<'a> {
    /// The arguments, in the order written.
    pub args: &'a [crate::ast::Expr],
    /// The name each is given — empty when none is.
    pub labels: &'a [Option<crate::ast::Label>],
    /// The whole call.
    pub span: Span,
    /// Its address in the module checked: what its binding is recorded by.
    pub at: usize,
}

impl FnInfo {
    /// A script's own callable, from its parameters.
    pub(crate) fn of_params(params: Vec<Ty>, result: Ty, declared: &[crate::ast::Param]) -> Self {
        Self {
            params,
            result,
            variadic: false,
            names: declared.iter().map(|p| p.name.clone()).collect(),
            defaults: declared.iter().map(|p| p.default.clone()).collect(),
        }
    }

    /// What a call to it is bound against.
    pub(crate) fn callee<'a>(&'a self, name: &'a str) -> Callee<'a> {
        Callee {
            name,
            params: &self.params,
            names: &self.names,
            defaults: &self.defaults,
        }
    }
}

/// What a parameter list belongs to, which decides whether its parameters may
/// have defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParamRole {
    /// A free function, a behavior's or a state's method.
    Callable,
    /// A member the engine calls.
    Lifecycle,
    /// `on Event(…)`.
    Handler,
    /// `event Name(…)`.
    Event,
    /// `state Name(…)`.
    State,
    /// `static T operator -(…)`.
    Operator,
}

impl Checker {
    /// Checks the arguments of a call to `callee` in the order written, each
    /// against the parameter it fills, and returns how they bind — `None` when
    /// the call binds as it always did.
    pub(super) fn bind_arguments(
        &mut self,
        callee: &Callee<'_>,
        args: &[Expr],
        labels: &[Option<Label>],
        span: Span,
        context: &Context,
    ) -> Option<Vec<Bound>> {
        let count = callee.params.len();
        let mut filled: Vec<Option<usize>> = vec![None; count];
        let mut target: Vec<Option<usize>> = vec![None; args.len()];
        let named = |at: usize| labels.get(at).and_then(Option::as_ref);
        let positional = (0..args.len())
            .take_while(|&at| named(at).is_none())
            .count();

        if positional > count || (labels.is_empty() && positional < callee.required()) {
            self.error(
                format!(
                    "`{}` takes {}, found {}",
                    callee.name,
                    callee.arity(),
                    args.len()
                ),
                span,
            );
        }
        for (at, slot) in target.iter_mut().enumerate().take(positional.min(count)) {
            *slot = Some(at);
            filled[at] = Some(at);
        }
        for (at, argument) in args.iter().enumerate().skip(positional) {
            let Some(label) = named(at) else {
                // A positional argument after a named one: refused by the
                // parser, checked only for its own mistakes.
                continue;
            };
            let written = label.span.to(argument.span());
            match callee.names.iter().position(|name| *name == label.name) {
                None => self.unknown_parameter(callee, &label.name, written),
                Some(index) if filled[index].is_some() => {
                    self.error(format!("`{}` is given twice", label.name), written)
                }
                Some(index) => {
                    filled[index] = Some(at);
                    target[at] = Some(index);
                }
            }
        }

        for (at, argument) in args.iter().enumerate() {
            let actual = self.check_expr(argument, context);
            if let Some(index) = target[at] {
                self.expect_assignable(&callee.params[index], &actual, argument.span());
            }
        }

        let mut bound = Vec::with_capacity(count);
        let mut complete = true;
        for (index, given) in filled.iter().enumerate() {
            match (given, callee.defaults.get(index).cloned().flatten()) {
                (Some(at), _) => bound.push(Bound::Written(*at)),
                (None, Some(default)) => {
                    let default = Box::new(default);
                    // Checked here, where it is passed: what the compiler asks
                    // of it — its type, how a constructor in it binds — is
                    // recorded at the very place it compiles from.
                    // Its mistakes were reported where it is declared.
                    if is_data_constant(&default) {
                        let reported = self.diagnostics.len();
                        let outer = std::mem::replace(&mut self.in_default, true);
                        self.check_expr(&default, context);
                        self.in_default = outer;
                        self.diagnostics.truncate(reported);
                    }
                    bound.push(Bound::Default(default));
                }
                (None, None) => {
                    complete = false;
                    // A positional call short of arguments was told so above.
                    if !labels.is_empty() {
                        let name = callee.names.get(index).map_or("a value", String::as_str);
                        self.error(format!("`{}` needs `{name}`", callee.name), span);
                    }
                }
            }
        }
        (complete && !callee.plain(args, labels)).then_some(bound)
    }

    /// A name `callee` has no parameter for: its parameters listed, the near
    /// one suggested.
    fn unknown_parameter(&mut self, callee: &Callee<'_>, name: &str, at: Span) {
        let listed = if callee.names.is_empty() {
            "it takes its arguments in order, unnamed".to_owned()
        } else {
            let names: Vec<String> = callee.names.iter().map(|n| format!("`{n}`")).collect();
            let near = callee
                .names
                .iter()
                .filter(|known| distance(known, name) <= 2)
                .min_by_key(|known| distance(known, name));
            match near {
                Some(near) => format!(
                    "its parameters are {} — did you mean `{near}`?",
                    names.join(", ")
                ),
                None => format!("its parameters are {}", names.join(", ")),
            }
        };
        self.error_note(
            format!("`{}` has no parameter `{name}`", callee.name),
            at,
            listed,
        );
    }

    /// Refuses the names given at a call to something that takes none.
    pub(super) fn refuse_labels(&mut self, name: &str, labels: &[Option<Label>]) {
        if let Some(first) = labels.iter().flatten().next() {
            self.error(format!("`{name}` takes no named argument"), first.span);
        }
    }

    /// The defaults of `params`, as `role` allows them: constants, of the
    /// parameter's type, none missing after the first — and none at all where
    /// the engine or a raise gives every value.
    pub(super) fn check_param_defaults(&mut self, params: &[Param], role: ParamRole) {
        let refused = match role {
            ParamRole::Callable => None,
            ParamRole::Lifecycle => Some((
                "a lifecycle method is called by the engine, which gives every argument",
                "remove the default; the engine always passes this value",
            )),
            ParamRole::Handler => Some((
                "a handler's parameters are its event's payload",
                "remove the default; every raise carries the whole payload",
            )),
            ParamRole::Event => Some((
                "an event's payload is given whole when raised",
                "remove the default; a raise names every value it carries",
            )),
            ParamRole::State => Some((
                "a state's data is given whole by `become`",
                "remove the default, and pass the value with `become`",
            )),
            ParamRole::Operator => Some((
                "an operator receives every operand",
                "remove the default; `a - b` always gives both",
            )),
        };
        let mut first_default: Option<&str> = None;
        for param in params {
            let Some(default) = &param.default else {
                if let Some(earlier) = first_default {
                    if refused.is_none() {
                        self.error_note(
                            format!(
                                "`{}` needs a default: it follows `{earlier}`, which has one",
                                param.name
                            ),
                            param.span,
                            "a call fills parameters in order, so every one after a default needs one too",
                        );
                    }
                }
                continue;
            };
            if let Some((message, note)) = refused {
                self.error_note(message, param.span, note);
                continue;
            }
            first_default.get_or_insert(&param.name);
            if !is_data_constant(default) {
                self.error_note(
                    "a parameter's default must be a constant",
                    default.span(),
                    "a default is passed by every call that leaves it out — write a number, text, `null`, a duration, an angle, an engine value of numbers (`Vec3(0.0, 1.0, 0.0)`), or `[…]` / `Name { … }` of those",
                );
                continue;
            }
            let expected = self.resolve(&param.ty);
            let outer = std::mem::replace(&mut self.in_default, true);
            let actual = self.check_expr(default, &Context::sync("a default", Ty::Void));
            self.in_default = outer;
            self.expect_assignable(&expected, &actual, default.span());
        }
    }
}

/// The edit distance between two names, for a suggestion.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let replaced = previous + usize::from(ca != *cb);
            previous = row[j + 1];
            row[j + 1] = replaced.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}
