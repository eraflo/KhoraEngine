// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Home screen state.

#[derive(Default)]
pub struct HomeState {
    /// Index of the currently hovered project card (in the source
    /// list, not the filtered view).
    pub hovered: Option<usize>,
    /// Free-text filter applied to project name + path.
    pub filter: String,
    /// Project pending a deletion confirmation modal.
    pub remove_confirm: Option<usize>,
}
