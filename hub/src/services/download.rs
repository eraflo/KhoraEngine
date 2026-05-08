// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Download and extraction of engine releases from GitHub.

use crate::config::EngineInstall;
use crate::github::GithubAsset;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;

/// Messages sent from the download thread to the UI.
pub enum DownloadMessage {
    /// Progress update: (bytes_downloaded, total_bytes).
    Progress(u64, u64),
    /// Download and extraction completed successfully.
    Completed {
        version: String,
        install: EngineInstall,
    },
    /// Download or extraction failed.
    Error(String),
}

/// Returns the base directory for engine installations: `~/.khora/engines/`.
fn engines_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".khora")
        .join("engines")
}

/// Returns the install directory for a specific engine version
/// (`~/.khora/engines/<version>/`). Public so the uninstaller can
/// locate the directory it needs to remove.
pub fn engine_install_dir(version: &str) -> PathBuf {
    engines_dir().join(version)
}

/// Removes a downloaded engine install from disk. Returns `Ok(())`
/// when the directory was successfully deleted (or never existed).
///
/// `Err` only on filesystem failure mid-deletion. Caller should
/// remove the matching entry from `HubConfig.engines` either way:
/// the on-disk directory and the config registration are tracked
/// separately, and a stale config entry is harmless.
pub fn uninstall_engine(version: &str) -> std::io::Result<()> {
    let dir = engine_install_dir(version);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Starts a background download of the editor archive, optionally also
/// fetching the matching `khora-runtime` archive into the same engine
/// cache slot.
///
/// Layout produced under `~/.khora/engines/<version>/`:
/// - `editor/` — extracted editor archive (contains `khora-editor{.exe}` + assets)
/// - `runtime/` — extracted runtime archive (contains `khora-runtime{.exe}`),
///   only when `runtime_asset` is `Some` and its download succeeds
///
/// `EngineInstall.runtime_binary` is populated when the runtime download
/// works; it stays `None` for older releases that don't ship the runtime
/// archive yet — the engine still installs and is usable for editing.
///
/// Returns a `Receiver` that the caller should poll every frame.
pub fn start_download(
    editor_asset: &GithubAsset,
    runtime_asset: Option<&GithubAsset>,
    version: &str,
) -> mpsc::Receiver<DownloadMessage> {
    let (tx, rx) = mpsc::channel();
    let editor_url = editor_asset.browser_download_url.clone();
    let runtime_url = runtime_asset.map(|a| a.browser_download_url.clone());
    let editor_size = editor_asset.size;
    let runtime_size = runtime_asset.map(|a| a.size).unwrap_or(0);
    let total_bytes = editor_size + runtime_size;
    let version = version.to_owned();
    let dest_root = engines_dir().join(&version);

    let _ = tx.send(DownloadMessage::Progress(0, total_bytes));

    std::thread::spawn(move || {
        let editor_dir = dest_root.join("editor");
        let runtime_dir = dest_root.join("runtime");

        // ── Editor ──────────────────────────────────────────────
        let editor_binary = match download_and_extract(
            &editor_url,
            &editor_dir,
            "khora-editor",
            editor_size,
            total_bytes,
            0,
            &tx,
        ) {
            Ok(p) => p,
            Err(e) => {
                let _ = tx.send(DownloadMessage::Error(format!("{e}")));
                return;
            }
        };

        // ── Runtime (best-effort) ───────────────────────────────
        let mut runtime_binary: Option<PathBuf> = None;
        if let Some(url) = runtime_url {
            match download_and_extract(
                &url,
                &runtime_dir,
                "khora-runtime",
                runtime_size,
                total_bytes,
                editor_size,
                &tx,
            ) {
                Ok(p) => {
                    runtime_binary = Some(p);
                }
                Err(e) => {
                    log::warn!("Runtime artifact download failed (engine still installed): {e}");
                }
            }
        }

        let install = EngineInstall {
            version: version.clone(),
            editor_binary: editor_binary.to_string_lossy().to_string(),
            runtime_binary: runtime_binary.map(|p| p.to_string_lossy().to_string()),
            source: "github".to_owned(),
        };
        let _ = tx.send(DownloadMessage::Completed { version, install });
    });

    rx
}

/// Downloads the file at `url` and extracts it (if zip) into `dest_dir`.
///
/// `bin_base_name` is the binary stem to look up after extraction
/// (`"khora-editor"`, `"khora-runtime"`, …) — `.exe` is appended on Windows.
/// `archive_size` is the size of *this* archive; `total_size` and
/// `progress_offset` let the caller report cumulative progress across
/// multiple downloads (editor + runtime in the same install slot).
///
/// Returns the absolute path to the resolved binary on success.
fn download_and_extract(
    url: &str,
    dest_dir: &std::path::Path,
    bin_base_name: &str,
    archive_size: u64,
    total_size: u64,
    progress_offset: u64,
    tx: &mpsc::Sender<DownloadMessage>,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;
    let _ = archive_size; // reserved for future fine-grained throttling

    // Build HTTP client
    let client = reqwest::blocking::Client::builder()
        .user_agent("khora-hub/0.1")
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("Failed to build HTTP client")?;

    // Download with progress reporting
    let response = client
        .get(url)
        .send()
        .context("Failed to send download request")?;

    if !response.status().is_success() {
        anyhow::bail!("Download returned status {}", response.status());
    }

    // Read in chunks to report progress
    let mut bytes = Vec::new();
    let mut reader = std::io::BufReader::new(response);
    let mut buf = [0u8; 65536];
    loop {
        let n = reader
            .read(&mut buf)
            .context("Failed to read download body")?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        let _ = tx.send(DownloadMessage::Progress(
            progress_offset + bytes.len() as u64,
            total_size,
        ));
    }

    // Create destination directory
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;

    let is_zip =
        url.ends_with(".zip") || bytes.len() >= 4 && bytes[0..4] == [0x50, 0x4B, 0x03, 0x04];

    if is_zip {
        extract_zip(&bytes, dest_dir)?;
    } else {
        // Save the raw file using a sensible default filename.
        let filename = url.rsplit('/').next().unwrap_or(bin_base_name);
        let dest_file = dest_dir.join(filename);
        std::fs::write(&dest_file, &bytes)
            .with_context(|| format!("Failed to write {}", dest_file.display()))?;

        // Make executable on unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dest_file)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&dest_file, perms)?;
        }
    }

    // Find the requested binary inside the extracted tree.
    let exe_name = if cfg!(windows) {
        format!("{bin_base_name}.exe")
    } else {
        bin_base_name.to_owned()
    };

    find_file_recursive(dest_dir, &exe_name)
        .with_context(|| format!("Could not find {} in {}", exe_name, dest_dir.display()))
}

/// Extracts a zip archive from bytes into `dest_dir`.
fn extract_zip(data: &[u8], dest_dir: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::Cursor;

    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).context("Failed to open zip archive")?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to read zip entry")?;
        let name = file.name().to_owned();

        // Prevent zip-slip: ensure no path traversal
        if name.contains("..") {
            log::warn!("Skipping suspicious zip entry: {}", name);
            continue;
        }

        let out_path = dest_dir.join(&name);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .with_context(|| format!("Failed to create {}", out_path.display()))?;
            std::io::copy(&mut file, &mut out_file)?;

            // Preserve executable permissions on unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    let mut perms = std::fs::metadata(&out_path)?.permissions();
                    perms.set_mode(mode);
                    std::fs::set_permissions(&out_path, perms)?;
                }
            }
        }
    }

    Ok(())
}

/// Recursively searches for a file by name in a directory tree.
fn find_file_recursive(dir: &std::path::Path, filename: &str) -> Option<PathBuf> {
    // Check the direct path first
    let direct = dir.join(filename);
    if direct.exists() {
        return Some(direct);
    }

    // Recurse
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, filename) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(filename) {
            return Some(path);
        }
    }
    None
}
