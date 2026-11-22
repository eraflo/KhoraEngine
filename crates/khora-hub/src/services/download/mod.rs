// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Download and extraction of engine releases from GitHub.

use crate::services::config::EngineInstall;
use crate::services::github::GithubAsset;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;

/// The most a download may hold when its size is not known.
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

/// The most one archive may unpack to: far past a release's editor and
/// runtime, far under a disk.
const MAX_UNPACKED: u64 = 4 * 1024 * 1024 * 1024;

/// Messages sent from the download thread to the UI.
pub enum DownloadMessage {
    /// Progress update: (`bytes_downloaded`, `total_bytes`).
    Progress(u64, u64),
    /// Download and extraction completed successfully.
    Completed {
        /// The release tag installed.
        version: String,
        /// The installation, to add to the hub config.
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

/// Returns the install directory of engine `version`: `~/.khora/engines/<version>/`.
pub(crate) fn engine_install_dir(version: &str) -> PathBuf {
    engines_dir().join(version)
}

/// Removes the downloaded engine `version` from disk; a missing one is no error.
///
/// The caller removes its entry from `HubConfig.engines` whatever the
/// outcome: a stale config entry is harmless.
///
/// # Errors
///
/// Fails when the filesystem refuses the deletion partway.
pub(crate) fn uninstall_engine(version: &str) -> std::io::Result<()> {
    let dir = engine_install_dir(version);
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Downloads engine `version` in the background, its runtime too when given.
///
/// The archives are extracted to `~/.khora/engines/<version>/editor/` and
/// `runtime/`. A runtime that fails leaves `EngineInstall.runtime_binary` at
/// `None`; the engine is still installed. Poll the returned `Receiver` every frame.
#[expect(
    clippy::disallowed_methods,
    reason = "the hub runs no engine: a network call runs on its own thread"
)]
pub(crate) fn start_download(
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

/// Downloads the file at `url` and extracts it (a zip or a gzipped tar) into `dest_dir`.
///
/// Returns the path of the binary `bin_base_name` (`.exe` added on Windows)
/// found in the extraction. `archive_size` is the size the release states for
/// the file (0 when unknown): a body larger than it, or than
/// [`MAX_DOWNLOAD`] when unknown, is refused. `total_size` and
/// `progress_offset` place this archive's progress within the whole install's.
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
    let most = if archive_size > 0 {
        archive_size
    } else {
        MAX_DOWNLOAD
    };

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
        if bytes.len() as u64 > most {
            anyhow::bail!("Download is larger than the {most} bytes the release states");
        }
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
    let is_tar_gz = url.ends_with(".tar.gz")
        || url.ends_with(".tgz")
        || bytes.len() >= 2 && bytes[0..2] == [0x1F, 0x8B];

    if is_zip {
        extract_zip(&bytes, dest_dir, MAX_UNPACKED)?;
    } else if is_tar_gz {
        extract_tar_gz(&bytes, dest_dir, MAX_UNPACKED)?;
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

/// Extracts a zip archive from bytes into `dest_dir`, refusing one that
/// unpacks to more than `most` bytes.
fn extract_zip(data: &[u8], dest_dir: &std::path::Path, most: u64) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::{Cursor, Read};

    let reader = Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader).context("Failed to open zip archive")?;
    let mut unpacked: u64 = 0;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to read zip entry")?;

        // Prevent zip-slip: an entry that climbs out of the archive (`..`) has
        // no enclosed name, and is skipped; a leading root is dropped, so an
        // absolute path lands inside `dest_dir`.
        let Some(relative) = file.enclosed_name() else {
            log::warn!(
                "Skipping suspicious zip entry: {}",
                file.name().unwrap_or_default()
            );
            continue;
        };

        let out_path = dest_dir.join(relative);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)
                .with_context(|| format!("Failed to create {}", out_path.display()))?;
            let left = most - unpacked;
            let copied = std::io::copy(&mut (&mut file).take(left + 1), &mut out_file)?;
            if copied > left {
                anyhow::bail!("Zip archive unpacks to more than {most} bytes");
            }
            unpacked += copied;

            // Preserve executable permissions on unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    let mut perms = std::fs::metadata(&out_path)?.permissions();
                    // Only the owner's bits and others' read/execute: an
                    // archive never grants setuid, setgid, sticky, nor a
                    // write to anyone but the owner.
                    perms.set_mode(mode & 0o755);
                    std::fs::set_permissions(&out_path, perms)?;
                }
            }
        }
    }

    Ok(())
}

/// Extracts a gzipped tar archive from bytes into `dest_dir`, refusing one
/// that unpacks to more than `most` bytes.
///
/// The Linux and macOS releases ship as `.tar.gz`. Only regular files are
/// unpacked, their folders made as they come: a link, a device or any other
/// entry is skipped, so nothing in the archive can point outside `dest_dir`.
/// An entry that climbs out (`..`) or holds a name Windows cannot keep
/// (see [`portable_name`]) is skipped; a leading root is dropped, so an
/// absolute path lands inside `dest_dir`. A file keeps the owner's bits and
/// others' read/execute, never setuid, setgid, sticky nor a write by others.
fn extract_tar_gz(data: &[u8], dest_dir: &std::path::Path, most: u64) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::io::Read;

    // `unpack_in` checks each entry against the folder's real path, so it must exist.
    std::fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;
    // Past `most`, the stream ends early and the archive reads as cut short.
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(data).take(most));
    archive.set_preserve_permissions(false);
    archive.set_mask(0o022);
    archive.set_overwrite(true);
    for entry in archive.entries().context("Failed to open tar archive")? {
        let mut entry = entry.context("Failed to read tar entry")?;
        let path = entry
            .path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            log::warn!("Skipping tar entry that is not a file: {path}");
            continue;
        }
        let portable = entry
            .path()
            .map(|p| p.components().all(portable_name))
            .unwrap_or(false);
        if !portable {
            log::warn!("Skipping tar entry with a name Windows cannot keep: {path}");
            continue;
        }
        let unpacked = entry
            .unpack_in(dest_dir)
            .with_context(|| format!("Failed to extract {path}"))?;
        if !unpacked {
            log::warn!("Skipping suspicious tar entry: {path}");
        }
    }
    Ok(())
}

/// Returns whether a path component is one every OS keeps as written.
///
/// Refused: a component holding `:` (a Windows stream or drive), ending in a
/// dot or a space, or a Windows device name (`CON`, `PRN`, `AUX`, `NUL`,
/// `COM1`-`COM9`, `LPT1`-`LPT9`, with any extension). Other kinds of
/// component (`..`, a root) are left to `unpack_in`.
fn portable_name(component: std::path::Component<'_>) -> bool {
    let std::path::Component::Normal(name) = component else {
        return true;
    };
    let Some(name) = name.to_str() else {
        return false;
    };
    if name.contains(':') || name.ends_with('.') || name.ends_with(' ') {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    !device
}

/// Searches a directory tree, recursively, for a file by name.
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

#[cfg(test)]
mod tests;
