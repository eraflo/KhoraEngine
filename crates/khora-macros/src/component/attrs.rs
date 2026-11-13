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

//! The type-level `#[component(...)]` keys a derived component declares:
//! its layout, whether it is saved, whether a game save keeps it, its
//! domain, its provenance and its former names.
//!
//! The `check` key is read apart, with its errors kept
//! (`crate::component_check`).

use syn::DeriveInput;

/// What the type-level `#[component(...)]` attributes say.
pub(super) struct ComponentAttrs {
    /// `layout = "soa"`: stored as a field-split column.
    pub(super) layout_soa: bool,
    /// `no_serializable`: no mirror and no registration.
    pub(super) no_serializable: bool,
    /// `resumable`: runtime state a game save keeps.
    pub(super) resumable: bool,
    /// `domain = <SemanticDomain variant>`, if declared.
    pub(super) domain: Option<syn::Ident>,
    /// `provenance = <ComponentProvenance variant>`; `Authored` when absent.
    pub(super) provenance: syn::Ident,
    /// Each `formerly = "<old name>"`, in order.
    pub(super) formerly: Vec<syn::LitStr>,
}

impl ComponentAttrs {
    /// Reads the type-level keys of `input`.
    pub(super) fn of(input: &DeriveInput) -> Self {
        let (domain, provenance, formerly) = value_keys(input);
        Self {
            layout_soa: layout_soa(input),
            no_serializable: no_serializable(input),
            resumable: resumable(input),
            domain,
            provenance,
            formerly,
        }
    }
}

/// Whether the component opts into the field-SoA layout:
/// `#[component(layout = "soa")]`.
///
/// A component can declare a field-split (Structure-of-Arrays) physical
/// column — the AGDF compute-friendly layout. v1 supports structs whose
/// fields are all `f32`. When opted in, the generated `Component` impl routes
/// storage through a `FieldSoaColumn<Self>` and a `SoaLayout` impl describes
/// the field↔lane mapping. Otherwise the `Component` impl stays the default
/// (empty marker) and no `SoaLayout` is emitted.
///
/// As in the `no_serializable` scan, other `key = value` metas are
/// consumed so `layout` is found whatever the key order.
fn layout_soa(input: &DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("component") {
            return false;
        }
        let mut soa = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("layout") {
                let s = meta.value()?.parse::<syn::LitStr>()?;
                soa = s.value() == "soa";
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            }
            Ok(())
        });
        soa
    })
}

/// Whether the component carries `#[component(no_serializable)]`.
///
/// The `else if` branch consumes unknown `key = value` metas (`domain`,
/// `provenance`, `layout`, `formerly`) so the walk reaches this flag whatever the order
/// the keys were written in — without it, `parse_nested_meta` aborts on the
/// first unconsumed value and the discarded error makes the miss silent.
fn no_serializable(input: &DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("component") {
            return false;
        }
        let mut no = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("no_serializable") {
                no = true;
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            }
            Ok(())
        });
        no
    })
}

/// Whether the component carries `#[component(resumable)]`: state the engine
/// writes while running that a game save keeps so play resumes where it
/// stopped. Walked like `no_serializable`, consuming the `key = value` metas
/// it does not read.
fn resumable(input: &DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("component") {
            return false;
        }
        let mut found = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("resumable") {
                found = true;
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            }
            Ok(())
        });
        found
    })
}

/// The type-level `#[component(...)]` keys that carry a value:
///
/// * `domain = <SemanticDomain variant>` — the component self-registers its
///   domain into `World` via `inventory`, so the domain lives on the type
///   instead of a hand-maintained list in `World::new`.
/// * `provenance = <ComponentProvenance variant>` — who writes the component.
///   Absent means `Authored`: a component is the author's data unless it says
///   otherwise, so engine-written types opt out explicitly rather than every
///   authored type having to opt in.
/// * `formerly = "<old name>"` (repeatable) — a name the type was registered
///   under before: a save that names it so still loads, reported as renamed.
///
/// All value keys are read in ONE pass. `parse_nested_meta` requires every meta it
/// walks to be fully consumed, so a closure that recognises only one key
/// aborts on the other's `= value` — and because the result is discarded, the
/// failure is silent and the second key is simply never seen. Hence the
/// trailing branch that consumes unknown `key = value` pairs (`layout = ...`)
/// so parsing continues past them.
fn value_keys(input: &DeriveInput) -> (Option<syn::Ident>, syn::Ident, Vec<syn::LitStr>) {
    let mut domain = None;
    let mut provenance = None;
    let mut formerly = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("component") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("domain") {
                domain = Some(meta.value()?.parse::<syn::Ident>()?);
            } else if meta.path.is_ident("provenance") {
                provenance = Some(meta.value()?.parse::<syn::Ident>()?);
            } else if meta.path.is_ident("formerly") {
                formerly.push(meta.value()?.parse::<syn::LitStr>()?);
            } else if meta.input.peek(syn::Token![=]) {
                let _ = meta.value()?.parse::<syn::Expr>()?;
            }
            Ok(())
        });
    }
    let provenance = provenance
        .unwrap_or_else(|| syn::Ident::new("Authored", proc_macro::Span::call_site().into()));
    (domain, provenance, formerly)
}
