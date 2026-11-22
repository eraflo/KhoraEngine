// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Tests of the downloads: extraction of zip and gzipped tar archives, its
//! refusals and limits, and a download served from a local port.

use std::io::{Read, Write};

use super::{download_and_extract, extract_zip};

/// A scratch directory that removes itself, so a failing assertion cannot
/// leave extracted files behind for the next run to trip over.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("khora-hub-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir is writable");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Builds a deflate-compressed archive in memory, holding a nested file and
/// a path-traversal entry.
///
/// Deflate specifically: it is what real release archives use, and a `zip`
/// upgrade that dropped the codec would still *compile* — this is the test
/// that would notice.
fn sample_archive() -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer
            .start_file("nested/hello.txt", options)
            .expect("entry starts");
        writer.write_all(b"khora").expect("entry is writable");
        writer
            .start_file("../escape.txt", options)
            .expect("entry starts");
        writer.write_all(b"nope").expect("entry is writable");
        writer.finish().expect("archive finishes");
    }
    cursor.into_inner()
}

/// Returns the CRC-32 (IEEE) of `bytes`, as gzip stores it.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Builds a `.tar.gz` holding one executable file `name`, as the release
/// workflow's `tar czf` packs the Linux and macOS archives.
fn tar_gz(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut header = [0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..108].copy_from_slice(b"0000755\0");
    header[108..116].copy_from_slice(b"0000000\0");
    header[116..124].copy_from_slice(b"0000000\0");
    header[124..136].copy_from_slice(format!("{:011o}\0", contents.len()).as_bytes());
    header[136..148].copy_from_slice(b"00000000000\0");
    header[148..156].copy_from_slice(b"        ");
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let sum: u32 = header.iter().map(|b| u32::from(*b)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut tar = header.to_vec();
    tar.extend_from_slice(contents);
    tar.resize(tar.len().div_ceil(512) * 512 + 1024, 0);

    // One stored (uncompressed) deflate block: still a valid gzip stream.
    let len = u16::try_from(tar.len()).expect("the tar fits one stored block");
    let mut gz = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff, 1];
    gz.extend_from_slice(&len.to_le_bytes());
    gz.extend_from_slice(&(!len).to_le_bytes());
    gz.extend_from_slice(&tar);
    gz.extend_from_slice(&crc32(&tar).to_le_bytes());
    gz.extend_from_slice(&u32::from(len).to_le_bytes());
    gz
}

/// Serves `body` once over HTTP on a local port; returns its URL, ending in `file`.
#[expect(
    clippy::disallowed_methods,
    reason = "a test serves the download it makes on a thread of its own"
)]
fn serve_once(body: Vec<u8>, file: &str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a local port");
    let port = listener.local_addr().expect("its address").port();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
            request.push(byte[0]);
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
    });
    format!("http://127.0.0.1:{port}/{file}")
}

#[test]
fn extracts_a_nested_deflated_entry() {
    let scratch = Scratch::new("extract");
    extract_zip(&sample_archive(), &scratch.0, super::MAX_UNPACKED).expect("archive extracts");

    let extracted = scratch.0.join("nested").join("hello.txt");
    assert!(extracted.is_file(), "missing {}", extracted.display());
    assert_eq!(
        std::fs::read_to_string(&extracted).expect("file is readable"),
        "khora"
    );
}

/// An entry pointing outside the destination is skipped rather than
/// followed — the whole archive still extracts, minus that entry.
#[test]
fn refuses_to_escape_the_destination() {
    let scratch = Scratch::new("zipslip");
    extract_zip(&sample_archive(), &scratch.0, super::MAX_UNPACKED).expect("archive extracts");

    let escaped = scratch
        .0
        .parent()
        .expect("temp dir has a parent")
        .join("escape.txt");
    assert!(
        !escaped.exists(),
        "zip-slip wrote outside the destination: {}",
        escaped.display()
    );
}

/// An archive's executable bit survives extraction; its setuid, setgid
/// and sticky bits never do.
#[cfg(unix)]
#[test]
fn an_extracted_file_keeps_its_permissions_but_never_setuid() {
    use std::os::unix::fs::PermissionsExt;

    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o4755);
        writer
            .start_file("bin/tool", options)
            .expect("entry starts");
        writer.write_all(b"#!/bin/sh\n").expect("entry is writable");
        writer.finish().expect("archive finishes");
    }
    let scratch = Scratch::new("setuid");
    extract_zip(&cursor.into_inner(), &scratch.0, super::MAX_UNPACKED).expect("archive extracts");

    let mode = std::fs::metadata(scratch.0.join("bin").join("tool"))
        .expect("file was extracted")
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o755, "mode was {mode:o}");
}

/// **A `.tar.gz` engine archive installs.**
///
/// The release workflow packs the Linux and macOS archives with `tar czf`,
/// and the hub offers exactly those names: one downloaded is extracted.
#[test]
fn a_tar_gz_engine_archive_is_extracted() {
    let exe = if cfg!(windows) {
        "khora-editor.exe"
    } else {
        "khora-editor"
    };
    let url = serve_once(tar_gz(exe, b"editor"), "khora-engine-linux-x86_64.tar.gz");
    let scratch = Scratch::new("targz");
    let (tx, _rx) = std::sync::mpsc::channel();
    let found = download_and_extract(&url, &scratch.0, "khora-editor", 0, 0, 0, &tx)
        .expect("the archive the release workflow ships installs");
    assert_eq!(
        std::fs::read(&found).expect("the binary is readable"),
        b"editor"
    );
}

/// Gzips a tar archive holding `entries`, each a header made by the caller
/// and its bytes, written as the archive holds them.
fn tar_gz_of(entries: Vec<(tar::Header, Vec<u8>)>) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);
    for (mut header, bytes) in entries {
        header.set_size(bytes.len() as u64);
        header.set_cksum();
        builder
            .append(&header, bytes.as_slice())
            .expect("entry appends");
    }
    builder
        .into_inner()
        .expect("tar finishes")
        .finish()
        .expect("gzip finishes")
}

/// Returns a header named by `raw_name`'s bytes as written, unchecked.
fn header_named(raw_name: &[u8], kind: tar::EntryType) -> tar::Header {
    let mut header = tar::Header::new_old();
    header.as_old_mut().name[..raw_name.len()].copy_from_slice(raw_name);
    header.set_entry_type(kind);
    header.set_mode(0o644);
    header
}

/// **A tar entry that climbs out of the install folder is not extracted.**
#[test]
fn a_tar_entry_climbing_out_of_the_folder_is_skipped() {
    let scratch = Scratch::new("tar-climb");
    let inside = scratch.0.join("inside");
    let archive = tar_gz_of(vec![
        (
            header_named(b"../escaped.txt", tar::EntryType::Regular),
            b"out".to_vec(),
        ),
        (
            header_named(b"kept.txt", tar::EntryType::Regular),
            b"in".to_vec(),
        ),
    ]);

    super::extract_tar_gz(&archive, &inside, super::MAX_UNPACKED).expect("the archive extracts");

    assert!(
        !scratch.0.join("escaped.txt").exists(),
        "an entry escaped the folder"
    );
    assert_eq!(std::fs::read(inside.join("kept.txt")).expect("kept"), b"in");
}

/// **A link in a tar archive is never made**, so no later entry can be
/// written through it outside the install folder.
#[test]
fn a_tar_link_is_not_made() {
    let scratch = Scratch::new("tar-link");
    let inside = scratch.0.join("inside");
    let mut link = header_named(b"out", tar::EntryType::Symlink);
    link.set_link_name("..").expect("a link name");
    let archive = tar_gz_of(vec![
        (link, Vec::new()),
        (
            header_named(b"out/escaped.txt", tar::EntryType::Regular),
            b"out".to_vec(),
        ),
    ]);

    super::extract_tar_gz(&archive, &inside, super::MAX_UNPACKED).expect("the archive extracts");

    assert!(
        !scratch.0.join("escaped.txt").exists(),
        "an entry went through the link"
    );
    let out = inside.join("out");
    assert!(
        std::fs::symlink_metadata(&out).map_or(true, |m| !m.file_type().is_symlink()),
        "the link was made"
    );
}

/// **A tar file keeps its execute bits, never setuid.**
#[cfg(unix)]
#[test]
fn a_tar_file_keeps_its_execute_bits_but_never_setuid() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("tar-mode");
    let mut tool = header_named(b"tool", tar::EntryType::Regular);
    tool.set_mode(0o4755);
    let archive = tar_gz_of(vec![(tool, b"#!".to_vec())]);

    super::extract_tar_gz(&archive, &scratch.0, super::MAX_UNPACKED).expect("the archive extracts");

    let mode = std::fs::metadata(scratch.0.join("tool"))
        .expect("extracted")
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o755, "mode was {mode:o}");
}

/// **A tar archive unpacking past its limit is refused.**
#[test]
fn a_tar_archive_unpacking_past_its_limit_is_refused() {
    let scratch = Scratch::new("tar-bomb");
    let archive = tar_gz_of(vec![(
        header_named(b"big.bin", tar::EntryType::Regular),
        vec![0u8; 64 * 1024],
    )]);

    let result = super::extract_tar_gz(&archive, &scratch.0, 4 * 1024);

    assert!(result.is_err(), "an archive past its limit unpacked");
}

/// **A zip archive unpacking past its limit is refused.**
#[test]
fn a_zip_archive_unpacking_past_its_limit_is_refused() {
    let scratch = Scratch::new("zip-bomb");
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        writer
            .start_file("big.bin", zip::write::SimpleFileOptions::default())
            .expect("entry starts");
        writer.write_all(&[0u8; 64 * 1024]).expect("entry writes");
        writer.finish().expect("zip finishes");
    }

    let result = super::extract_zip(&cursor.into_inner(), &scratch.0, 4 * 1024);

    assert!(result.is_err(), "an archive past its limit unpacked");
}

/// **A download larger than the size its release states is refused.**
#[test]
fn a_download_larger_than_stated_is_refused() {
    let url = serve_once(vec![0u8; 4096], "khora-engine-linux-x86_64.tar.gz");
    let scratch = Scratch::new("too-big");
    let (tx, _rx) = std::sync::mpsc::channel();

    let result = download_and_extract(&url, &scratch.0, "khora-editor", 1024, 0, 0, &tx);

    let error = result.expect_err("a body past the stated size was kept");
    assert!(
        error.to_string().contains("larger than"),
        "the refusal says why: {error}"
    );
}

/// **A tar entry named as Windows cannot keep it is skipped.**
#[test]
fn a_tar_entry_with_a_name_windows_cannot_keep_is_skipped() {
    let scratch = Scratch::new("tar-names");
    let archive = tar_gz_of(vec![
        (
            header_named(b"editor:stream", tar::EntryType::Regular),
            b"x".to_vec(),
        ),
        (
            header_named(b"bin/NUL.txt", tar::EntryType::Regular),
            b"x".to_vec(),
        ),
        (
            header_named(b"trailing.", tar::EntryType::Regular),
            b"x".to_vec(),
        ),
        (
            header_named(b"COM1", tar::EntryType::Regular),
            b"x".to_vec(),
        ),
        (
            header_named(b"kept.txt", tar::EntryType::Regular),
            b"in".to_vec(),
        ),
    ]);

    super::extract_tar_gz(&archive, &scratch.0, super::MAX_UNPACKED).expect("the archive extracts");

    let mut names: Vec<String> = std::fs::read_dir(&scratch.0)
        .expect("the folder reads")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["kept.txt".to_owned()]);
}

/// **A tar entry named from the root lands inside the folder.**
#[test]
fn a_tar_entry_named_from_the_root_lands_inside_the_folder() {
    let scratch = Scratch::new("tar-root");
    let archive = tar_gz_of(vec![(
        header_named(b"/rooted/file.txt", tar::EntryType::Regular),
        b"in".to_vec(),
    )]);

    super::extract_tar_gz(&archive, &scratch.0, super::MAX_UNPACKED).expect("the archive extracts");

    assert_eq!(
        std::fs::read(scratch.0.join("rooted").join("file.txt")).expect("landed inside"),
        b"in"
    );
}

/// **A hard link in a tar archive is never made.**
#[test]
fn a_tar_hard_link_is_not_made() {
    let scratch = Scratch::new("tar-hardlink");
    let mut link = header_named(b"linked.txt", tar::EntryType::Link);
    link.set_link_name("kept.txt").expect("a link name");
    let archive = tar_gz_of(vec![
        (
            header_named(b"kept.txt", tar::EntryType::Regular),
            b"in".to_vec(),
        ),
        (link, Vec::new()),
    ]);

    super::extract_tar_gz(&archive, &scratch.0, super::MAX_UNPACKED).expect("the archive extracts");

    assert!(
        !scratch.0.join("linked.txt").exists(),
        "the hard link was made"
    );
}

/// **A tar file is never left writable by others.**
#[cfg(unix)]
#[test]
fn a_tar_file_is_never_writable_by_others() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("tar-world");
    let mut tool = header_named(b"tool", tar::EntryType::Regular);
    tool.set_mode(0o777);
    let archive = tar_gz_of(vec![(tool, b"#!".to_vec())]);

    super::extract_tar_gz(&archive, &scratch.0, super::MAX_UNPACKED).expect("the archive extracts");

    let mode = std::fs::metadata(scratch.0.join("tool"))
        .expect("extracted")
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o755, "mode was {mode:o}");
}
