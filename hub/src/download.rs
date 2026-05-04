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

/// Starts a background download of the given GitHub asset.
///
/// Downloads to `~/.khora/engines/<version>/`, extracts if zip,
/// and registers the engine install in the config.
///
/// Returns a `Receiver` that the caller should poll every frame.
pub fn start_download(asset: &GithubAsset, version: &str) -> mpsc::Receiver<DownloadMessage> {
    let (tx, rx) = mpsc::channel();
    let url = asset.browser_download_url.clone();
    let total_size = asset.size;
    let version = version.to_owned();
    let dest_dir = engines_dir().join(&version);

    let _ = tx.send(DownloadMessage::Progress(0, total_size));

    std::thread::spawn(
        move || match download_and_extract(&url, &dest_dir, total_size, &tx) {
            Ok(editor_binary) => {
                let install = EngineInstall {
                    version: version.clone(),
                    editor_binary: editor_binary.to_string_lossy().to_string(),
                    source: "github".to_owned(),
                };
                let _ = tx.send(DownloadMessage::Completed { version, install });
            }
            Err(e) => {
                let _ = tx.send(DownloadMessage::Error(format!("{}", e)));
            }
        },
    );

    rx
}

/// Downloads the file at `url` and extracts it (if zip) into `dest_dir`.
///
/// Returns the path to the editor binary inside the extracted directory.
fn download_and_extract(
    url: &str,
    dest_dir: &std::path::Path,
    total_size: u64,
    tx: &mpsc::Sender<DownloadMessage>,
) -> anyhow::Result<PathBuf> {
    use anyhow::Context;

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
        let _ = tx.send(DownloadMessage::Progress(bytes.len() as u64, total_size));
    }

    // Create destination directory
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;

    let is_zip =
        url.ends_with(".zip") || bytes.len() >= 4 && bytes[0..4] == [0x50, 0x4B, 0x03, 0x04];

    if is_zip {
        extract_zip(&bytes, dest_dir)?;
    } else {
        // Save the raw file
        let filename = url.rsplit('/').next().unwrap_or("khora-editor");
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

    // Find the editor binary in the extracted directory
    let exe_name = if cfg!(windows) {
        "khora-editor.exe"
    } else {
        "khora-editor"
    };

    find_file_recursive(dest_dir, exe_name)
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
