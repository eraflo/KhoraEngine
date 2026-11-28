// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Top-of-screen info / error banner: one that says what the hub did expires,
//! one that shows a project's refusals stays until the user dismisses it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::ProjectRefusal;

/// A short, dismissable notification shown above the active screen.
pub struct Banner {
    pub message: String,
    pub is_error: bool,
    /// When it goes on its own; `None` for one that stays until dismissed.
    pub expires_at: Option<Instant>,
    /// The project whose refusals it shows, dismissed with it; `None` for a
    /// banner that shows none.
    pub refusals_of: Option<PathBuf>,
}

impl Banner {
    const DEFAULT_TTL: Duration = Duration::from_secs(5);

    pub(crate) fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_error: false,
            expires_at: Some(Instant::now() + Self::DEFAULT_TTL),
            refusals_of: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_error: true,
            expires_at: Some(Instant::now() + Self::DEFAULT_TTL * 2),
            refusals_of: None,
        }
    }

    /// Returns the banner of one launch of the project at `project`: its `refusals`, then what the launch said.
    ///
    /// Built for that launch alone, from the refusals still listed of its
    /// project and the launch's own `outcome`; never from an earlier banner.
    /// With a refusal, it is an error that stays until dismissed, and
    /// dismisses them with it; with none, it is `outcome`; `None` when there
    /// is nothing to say.
    pub(crate) fn of_launch<'a>(
        project: &Path,
        refusals: impl IntoIterator<Item = &'a ProjectRefusal>,
        outcome: Option<Banner>,
    ) -> Option<Self> {
        let refused: Vec<String> = refusals.into_iter().map(ProjectRefusal::shown).collect();
        if refused.is_empty() {
            return outcome;
        }
        let refused = refused.join("; ");
        let message = match outcome {
            Some(outcome) => format!("{refused} — {}", outcome.message),
            None => refused,
        };
        Some(Self {
            message,
            is_error: true,
            expires_at: None,
            refusals_of: Some(project.to_path_buf()),
        })
    }

    pub(crate) fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|at| Instant::now() >= at)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Banner, ProjectRefusal};

    /// A refusal of `ember`'s `project.json`.
    fn refused_json() -> ProjectRefusal {
        ProjectRefusal {
            project: "/projects/ember".into(),
            name: "ember".to_owned(),
            message: "project.json is not read: refused".to_owned(),
        }
    }

    /// **A launch's refusals are shown with what the launch said, and stay.**
    #[test]
    fn a_launchs_refusals_are_shown_with_what_it_said_and_stay() {
        let ember = Path::new("/projects/ember");
        let refusal = refused_json();

        let shown = Banner::of_launch(
            ember,
            [&refusal],
            Some(Banner::info("Opened 'ember' in editor.")),
        )
        .expect("a banner");

        assert!(shown.is_error);
        assert!(
            shown.message.contains("'ember': project.json is not read"),
            "{}",
            shown.message
        );
        assert!(
            shown.message.contains("Opened 'ember'"),
            "{}",
            shown.message
        );
        assert!(!shown.is_expired(), "it stays until the user dismisses it");
        assert_eq!(shown.expires_at, None);
        assert_eq!(shown.refusals_of.as_deref(), Some(ember));

        let alone = Banner::of_launch(ember, [&refusal], None).expect("a banner");
        assert_eq!(alone.message, "'ember': project.json is not read: refused");
    }

    /// **A launch with no refusal says what it said, or nothing.**
    #[test]
    fn a_launch_with_no_refusal_says_what_it_said_or_nothing() {
        let ember = Path::new("/projects/ember");

        let said = Banner::of_launch(ember, [], Some(Banner::info("Opened 'ember' in editor.")))
            .expect("the launch's banner");

        assert_eq!(said.message, "Opened 'ember' in editor.");
        assert!(said.expires_at.is_some(), "it expires");
        assert_eq!(said.refusals_of, None);
        assert!(Banner::of_launch(ember, [], None).is_none());
    }
}
