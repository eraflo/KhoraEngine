// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub chrome — top bar, status bar, banner overlay.

pub mod banner;
pub mod status_bar;
pub mod title_bar;

pub use banner::paint_banner;
pub use status_bar::{STATUS_HEIGHT, show_status_bar};
pub use title_bar::{TITLE_BAR_HEIGHT, show_title_bar};
