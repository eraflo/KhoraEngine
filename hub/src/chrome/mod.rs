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
pub mod topbar;

pub use banner::paint_banner;
pub use status_bar::show_status_bar;
pub use topbar::show_topbar;
