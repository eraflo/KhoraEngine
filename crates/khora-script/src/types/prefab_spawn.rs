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

//! `Spawn(…)`, in its two forms: an entity at a position, or an instance of a
//! prefab — told apart by what the first argument is.

use khora_core::asset::AssetKind;

use crate::ast::{Expr, Label};
use crate::diagnostics::Span;

use super::{Checker, Context, Ty};

/// Whether `ty` is a prefab, or one that may be `null`.
pub(crate) fn is_prefab(ty: &Ty) -> bool {
    match ty {
        Ty::Asset(AssetKind::Prefab) => true,
        Ty::Optional(inner) => matches!(**inner, Ty::Asset(AssetKind::Prefab)),
        _ => false,
    }
}

impl Checker {
    /// `Spawn(position, C { … }, …)` or `Spawn(prefab, C { … }, …, at: p,
    /// facing: q)` — the new entity, or the instance's root, named at once.
    pub(super) fn check_any_spawn(
        &mut self,
        args: &[Expr],
        labels: &[Option<Label>],
        span: Span,
        context: &Context,
    ) -> Ty {
        let Some(first) = args.first() else {
            self.refuse_labels("Spawn", labels);
            return self.check_spawn(args, None, span, context);
        };
        let ty = self.check_expr(first, context);
        if !is_prefab(&ty) {
            self.refuse_labels("Spawn", labels);
            return self.check_spawn(args, Some(ty), span, context);
        }

        // The prefab is given first, by position: it names no parameter.
        if let Some(label) = labels.first().and_then(Option::as_ref) {
            self.error_note(
                format!("`Spawn` of a prefab takes no `{}:`", label.name),
                label.span,
                "the prefab comes first, unnamed — `Spawn(bullet, at: muzzle)`",
            );
        }
        let mut seen: Vec<&str> = Vec::new();
        let mut named: Vec<&str> = Vec::new();
        let mut placed = false;
        for (at, argument) in args.iter().enumerate().skip(1) {
            let label = labels.get(at).and_then(Option::as_ref);
            if let Some(label) =
                label.filter(|label| matches!(label.name.as_str(), "at" | "facing"))
            {
                if named.contains(&label.name.as_str()) {
                    self.error(format!("`{}:` is given twice", label.name), label.span);
                }
                named.push(label.name.as_str());
            }
            match label {
                None => {
                    if let Some(name) = self.check_patch(argument, "Spawn", context) {
                        if seen.contains(&name) {
                            self.error(
                                format!("`{name}` is attached twice: an entity holds one of each"),
                                argument.span(),
                            );
                        }
                        seen.push(name);
                    }
                }
                Some(label) if label.name == "at" => {
                    let found = self.check_expr(argument, context);
                    self.expect_assignable(&Ty::Engine("Vec3"), &found, argument.span());
                    placed = true;
                }
                Some(label) if label.name == "facing" => {
                    let found = self.check_expr(argument, context);
                    self.expect_assignable(&Ty::Engine("Quat"), &found, argument.span());
                }
                Some(label) => {
                    self.check_expr(argument, context);
                    self.error_note(
                        format!("`Spawn` of a prefab takes no `{}:`", label.name),
                        label.span,
                        "it takes `at:` — where the root stands — and `facing:` — how it turns",
                    );
                }
            }
        }
        if !placed {
            self.error_note(
                "`Spawn` of a prefab needs `at:`",
                span,
                "write where its root stands — `Spawn(bullet, at: muzzle)`",
            );
        }
        Ty::Entity
    }
}
