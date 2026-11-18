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

//! The keys a `#[component(...)]` attribute takes, where it sits, and the
//! checks every key goes through: known at its site, given once unless
//! repeatable, and holding the value it needs or none.

use quote::ToTokens;
use syn::meta::ParseNestedMeta;
use syn::parse::Parse;

/// The keys of a component's type-level `#[component(...)]`.
const TYPE_KEYS: &[&str] = &[
    "domain",
    "provenance",
    "check",
    "layout",
    "formerly",
    "resumable",
    "no_serializable",
];

/// The keys of a field's `#[component(...)]`.
const FIELD_KEYS: &[&str] = &["skip", "formerly"];

/// The keys that may be given more than once, each time with its own value.
const REPEATABLE: &[&str] = &["formerly"];

/// Where a `#[component(...)]` attribute sits, which decides the keys it takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Site {
    /// On the component's type.
    Type,
    /// On one of its fields.
    Field,
}

impl Site {
    /// Returns the keys an attribute at this site takes.
    fn keys(self) -> &'static [&'static str] {
        match self {
            Site::Type => TYPE_KEYS,
            Site::Field => FIELD_KEYS,
        }
    }

    /// Returns how a message names this site.
    fn place(self) -> &'static str {
        match self {
            Site::Type => "the type",
            Site::Field => "a field",
        }
    }

    /// Returns the other site.
    fn other(self) -> Site {
        match self {
            Site::Type => Site::Field,
            Site::Field => Site::Type,
        }
    }
}

/// The keys read so far from one item's `#[component(...)]` attributes.
pub(super) struct Keys {
    site: Site,
    /// Each key read, by name, with the path it was written as.
    seen: Vec<(String, syn::Path)>,
}

impl Keys {
    /// Returns an empty record of the keys read at `site`.
    pub(super) fn new(site: Site) -> Self {
        Self {
            site,
            seen: Vec::new(),
        }
    }

    /// Returns the name of the key `meta` holds, and records it.
    ///
    /// # Errors
    ///
    /// On the key, when this site does not take it (the message lists the
    /// keys it takes, and says so when the key belongs to the other site), or
    /// when a key that is not repeatable was read before.
    pub(super) fn key(&mut self, meta: &ParseNestedMeta) -> syn::Result<String> {
        let name = meta.path.to_token_stream().to_string().replace(' ', "");
        if !self.site.keys().contains(&name.as_str()) {
            let listed = self
                .site
                .keys()
                .iter()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let other = self.site.other();
            let elsewhere = if other.keys().contains(&name.as_str()) {
                format!("; `{name}` goes on {}", other.place())
            } else {
                String::new()
            };
            return Err(meta.error(format!(
                "unknown `#[component]` key `{name}` on {}: its keys are {listed}{elsewhere}",
                self.site.place()
            )));
        }
        if !REPEATABLE.contains(&name.as_str()) && self.written(&name).is_some() {
            return Err(meta.error(format!("`{name}` is given twice: keep one")));
        }
        self.seen.push((name.clone(), meta.path.clone()));
        Ok(name)
    }

    /// Returns the path `key` was first written as, if it was read.
    pub(super) fn written(&self, key: &str) -> Option<&syn::Path> {
        self.seen
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, path)| path)
    }
}

/// Checks that the flag `key` in `meta` carries no value.
///
/// # Errors
///
/// On the key, when a `= value` or a `(...)` follows it.
pub(super) fn flag(meta: &ParseNestedMeta, key: &str) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) || meta.input.peek(syn::token::Paren) {
        return Err(meta.error(format!("`{key}` takes no value: `#[component({key})]`")));
    }
    Ok(())
}

/// Returns the value of `key` in `meta`, parsed as a `T`.
///
/// `shape` says what the value is and `example` how it is written; both go in
/// the message when the value is missing or of another kind.
///
/// # Errors
///
/// On the key when it has no `= value`, on the value when it is not a `T`.
pub(super) fn value<T: Parse>(
    meta: &ParseNestedMeta,
    key: &str,
    shape: &str,
    example: &str,
) -> syn::Result<T> {
    let message = format!("`{key}` takes {shape}: `{example}`");
    if !meta.input.peek(syn::Token![=]) {
        return Err(meta.error(message));
    }
    meta.value()?
        .parse::<T>()
        .map_err(|error| syn::Error::new(error.span(), message))
}

/// Returns the name of a `formerly = "<old name>"` key, checked against the `earlier` ones.
///
/// Shared by the type and its fields: both rename the same way.
///
/// # Errors
///
/// On the value when it is not a string, is empty, or repeats an earlier
/// name; on the key when it has no value.
pub(super) fn former_name(
    meta: &ParseNestedMeta,
    earlier: &[syn::LitStr],
) -> syn::Result<syn::LitStr> {
    let former: syn::LitStr = value(
        meta,
        "formerly",
        "the old name, a string",
        "formerly = \"Old\"",
    )?;
    if former.value().is_empty() {
        return Err(syn::Error::new(
            former.span(),
            "`formerly` names an old name: it cannot be empty",
        ));
    }
    if earlier.iter().any(|name| name.value() == former.value()) {
        return Err(syn::Error::new(
            former.span(),
            format!(
                "`formerly = \"{}\"` is given twice: keep one",
                former.value()
            ),
        ));
    }
    Ok(former)
}
