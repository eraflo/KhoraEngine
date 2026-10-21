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

//! Embeds the editor's icon in the Windows executable.
//!
//! Explorer, a pinned taskbar button and the Start menu read the icon from
//! the executable itself, not from the window. A machine without a resource
//! compiler still builds the editor, without the icon, and says so.

const ICON: &str = "../khora-tool-ui/assets/khora-editor.ico";

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICON);
    if let Err(e) = res.compile() {
        println!("cargo:warning=the editor's executable icon was not embedded: {e}");
    }
}
