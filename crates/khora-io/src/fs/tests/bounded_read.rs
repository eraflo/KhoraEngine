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

//! Tests of reading a file under a cap.

use super::*;

#[test]
fn read_bounded_reads_a_file_at_its_cap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin");
    std::fs::write(&path, [7u8; 16]).unwrap();
    assert_eq!(read_bounded(&path, 16).unwrap(), vec![7u8; 16]);
}

#[test]
fn read_bounded_refuses_a_file_past_its_cap_with_its_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.bin");
    std::fs::write(&path, [7u8; 17]).unwrap();
    match read_bounded(&path, 16) {
        Err(BoundedReadError::TooLarge { size, cap, stamp }) => {
            assert_eq!((size, cap), (17, 16));
            assert_eq!(stamp, FileStamp::of(&path));
        }
        other => panic!("refused as too large, got {other:?}"),
    }
}

#[test]
fn read_bounded_says_a_missing_file_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let error = read_bounded(&dir.path().join("gone"), 16).unwrap_err();
    assert!(error.is_not_found());
}

#[test]
fn read_bounded_to_string_reads_text_and_refuses_what_is_not_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("a.txt");
    std::fs::write(&text, "héllo").unwrap();
    assert_eq!(read_bounded_to_string(&text, 64).unwrap(), "héllo");
    let binary = dir.path().join("a.bin");
    std::fs::write(&binary, [0xFF, 0xFE]).unwrap();
    match read_bounded_to_string(&binary, 64) {
        Err(BoundedReadError::Io(error)) => {
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData)
        }
        other => panic!("not UTF-8 is invalid data, got {other:?}"),
    }
}
