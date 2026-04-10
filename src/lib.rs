#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! `clip2preview` provides a small API surface for extracting embedded preview
//! images from `.clip` files.
//!
//! The current implementation scans the `.clip` container for an embedded
//! SQLite database, reads the `CanvasPreview` table, and returns the first
//! highest-resolution preview image it finds.

mod error;
mod parser;
mod preview;

pub use crate::error::{ClipError, Result};
pub use crate::preview::{Preview, PreviewFormat};

use std::fs::File;
use std::io::BufReader;
use std::io::{Read, Seek};
use std::path::Path;

/// Extracts the embedded preview image from a `.clip` file on disk.
pub fn extract_preview<P: AsRef<Path>>(path: P) -> Result<Preview> {
    let file = File::open(path)?;
    // The parser revisits small headers near large block reads, so buffering avoids extra syscalls.
    let mut reader = BufReader::new(file);
    extract_preview_from_reader(&mut reader)
}

/// Extracts the embedded preview image from any seekable reader.
pub fn extract_preview_from_reader<R: Read + Seek>(reader: &mut R) -> Result<Preview> {
    parser::ClipParser::new(reader).extract_preview()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn empty_input_is_rejected() {
        let mut input = Cursor::new(Vec::<u8>::new());
        let error = extract_preview_from_reader(&mut input).unwrap_err();

        assert!(matches!(error, ClipError::InvalidFormat("input is empty")));
    }

    #[test]
    fn non_clip_input_is_rejected() {
        let mut input = Cursor::new(vec![0x01, 0x02, 0x03]);
        let error = extract_preview_from_reader(&mut input).unwrap_err();

        assert!(matches!(
            error,
            ClipError::InvalidFormat("missing CSFCHUNK signature")
        ));
    }

    #[test]
    fn extracts_preview_from_embedded_sqlite() {
        let png_bytes = b"\x89PNG\r\n\x1a\nfake-png-payload".to_vec();
        let clip_bytes = build_synthetic_clip(&png_bytes, 2400, 1600);
        let mut input = Cursor::new(clip_bytes);

        let preview = extract_preview_from_reader(&mut input).unwrap();

        assert_eq!(preview.format(), PreviewFormat::Png);
        assert_eq!(preview.dimensions(), Some((2400, 1600)));
        assert_eq!(preview.bytes(), png_bytes.as_slice());
    }

    #[test]
    fn falls_back_to_sqlite_header_search_when_chunk_header_is_missing() {
        let png_bytes = b"\x89PNG\r\n\x1a\nfallback-payload".to_vec();
        let clip_bytes = build_legacy_synthetic_clip(&png_bytes, 1024, 768);
        let mut input = Cursor::new(clip_bytes);

        let preview = extract_preview_from_reader(&mut input).unwrap();

        assert_eq!(preview.format(), PreviewFormat::Png);
        assert_eq!(preview.dimensions(), Some((1024, 768)));
        assert_eq!(preview.bytes(), png_bytes.as_slice());
    }

    #[test]
    fn ignores_false_positive_chunk_headers_in_the_same_scan_window() {
        let png_bytes = b"\x89PNG\r\n\x1a\nchunk-header-false-positive".to_vec();
        let mut clip_bytes = build_synthetic_clip(&png_bytes, 320, 240);
        append_false_positive_chunk_header(&mut clip_bytes);
        append_false_positive_sqlite_header(&mut clip_bytes);
        let mut input = Cursor::new(clip_bytes);

        let preview = extract_preview_from_reader(&mut input).unwrap();

        assert_eq!(preview.format(), PreviewFormat::Png);
        assert_eq!(preview.dimensions(), Some((320, 240)));
        assert_eq!(preview.bytes(), png_bytes.as_slice());
    }

    #[test]
    fn ignores_false_positive_sqlite_headers_in_the_same_scan_window() {
        let png_bytes = b"\x89PNG\r\n\x1a\nsqlite-header-false-positive".to_vec();
        let mut clip_bytes = build_legacy_synthetic_clip(&png_bytes, 800, 600);
        append_false_positive_sqlite_header(&mut clip_bytes);
        let mut input = Cursor::new(clip_bytes);

        let preview = extract_preview_from_reader(&mut input).unwrap();

        assert_eq!(preview.format(), PreviewFormat::Png);
        assert_eq!(preview.dimensions(), Some((800, 600)));
        assert_eq!(preview.bytes(), png_bytes.as_slice());
    }

    #[test]
    fn returns_preview_not_found_when_canvas_preview_has_no_rows() {
        let clip_bytes = build_synthetic_clip_without_preview();
        let mut input = Cursor::new(clip_bytes);
        let error = extract_preview_from_reader(&mut input).unwrap_err();

        assert!(matches!(error, ClipError::PreviewNotFound));
    }

    #[test]
    fn preview_can_be_written_to_disk() {
        let preview = Preview::new(PreviewFormat::Png, vec![1, 2, 3, 4]);
        let path = unique_temp_path("clip2preview-preview.bin");

        preview.save(&path).unwrap();

        let saved = fs::read(&path).unwrap();
        assert_eq!(saved, vec![1, 2, 3, 4]);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn extract_preview_reads_preview_from_path() {
        let png_bytes = b"\x89PNG\r\n\x1a\npath-extract".to_vec();
        let clip_bytes = build_synthetic_clip(&png_bytes, 640, 480);
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), clip_bytes).unwrap();

        let preview = extract_preview(file.path()).unwrap();

        assert_eq!(preview.format(), PreviewFormat::Png);
        assert_eq!(preview.dimensions(), Some((640, 480)));
        assert_eq!(preview.bytes(), png_bytes.as_slice());
    }

    #[test]
    fn extract_preview_rereads_modified_path() {
        let png_bytes = b"\x89PNG\r\n\x1a\nreread-path".to_vec();
        let clip_bytes = build_synthetic_clip(&png_bytes, 333, 444);
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), clip_bytes).unwrap();

        let preview = extract_preview(file.path()).unwrap();
        assert_eq!(preview.dimensions(), Some((333, 444)));

        fs::write(file.path(), b"bad").unwrap();

        let error = extract_preview(file.path()).unwrap_err();
        assert!(matches!(
            error,
            ClipError::InvalidFormat("missing CSFCHUNK signature")
        ));
    }

    fn unique_temp_path(file_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!("{nanos}-{file_name}"))
    }

    fn build_synthetic_clip(preview_bytes: &[u8], width: i64, height: i64) -> Vec<u8> {
        let db_bytes = build_embedded_database(|connection| {
            connection
                .execute_batch(
                    "CREATE TABLE CanvasPreview(
                        _PW_ID INTEGER PRIMARY KEY AUTOINCREMENT,
                        MainId INTEGER DEFAULT NULL,
                        CanvasId INTEGER DEFAULT NULL,
                        ImageType INTEGER DEFAULT NULL,
                        ImageWidth INTEGER DEFAULT NULL,
                        ImageHeight INTEGER DEFAULT NULL,
                        ImageData BLOB DEFAULT NULL
                    );",
                )
                .unwrap();

            connection
                .execute(
                    "INSERT INTO CanvasPreview
                     (MainId, CanvasId, ImageType, ImageWidth, ImageHeight, ImageData)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![1_i64, 1_i64, 1_i64, width, height, preview_bytes],
                )
                .unwrap();
        });

        wrap_database_in_clip_container(&db_bytes)
    }

    fn build_legacy_synthetic_clip(preview_bytes: &[u8], width: i64, height: i64) -> Vec<u8> {
        let db_bytes = build_embedded_database(|connection| {
            connection
                .execute_batch(
                    "CREATE TABLE CanvasPreview(
                        _PW_ID INTEGER PRIMARY KEY AUTOINCREMENT,
                        MainId INTEGER DEFAULT NULL,
                        CanvasId INTEGER DEFAULT NULL,
                        ImageType INTEGER DEFAULT NULL,
                        ImageWidth INTEGER DEFAULT NULL,
                        ImageHeight INTEGER DEFAULT NULL,
                        ImageData BLOB DEFAULT NULL
                    );",
                )
                .unwrap();

            connection
                .execute(
                    "INSERT INTO CanvasPreview
                     (MainId, CanvasId, ImageType, ImageWidth, ImageHeight, ImageData)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![1_i64, 1_i64, 1_i64, width, height, preview_bytes],
                )
                .unwrap();
        });

        wrap_database_in_legacy_clip_container(&db_bytes)
    }

    fn build_synthetic_clip_without_preview() -> Vec<u8> {
        let db_bytes = build_embedded_database(|connection| {
            connection
                .execute_batch(
                    "CREATE TABLE CanvasPreview(
                        _PW_ID INTEGER PRIMARY KEY AUTOINCREMENT,
                        MainId INTEGER DEFAULT NULL,
                        CanvasId INTEGER DEFAULT NULL,
                        ImageType INTEGER DEFAULT NULL,
                        ImageWidth INTEGER DEFAULT NULL,
                        ImageHeight INTEGER DEFAULT NULL,
                        ImageData BLOB DEFAULT NULL
                    );",
                )
                .unwrap();
        });

        wrap_database_in_clip_container(&db_bytes)
    }

    fn build_embedded_database(setup: impl FnOnce(&Connection)) -> Vec<u8> {
        let database = NamedTempFile::new().unwrap();
        let path = database.path().to_path_buf();

        {
            let connection = Connection::open(&path).unwrap();
            setup(&connection);
        }

        fs::read(path).unwrap()
    }

    fn wrap_database_in_clip_container(database_bytes: &[u8]) -> Vec<u8> {
        let mut clip_bytes = Vec::with_capacity(160 + database_bytes.len());
        clip_bytes.extend_from_slice(b"CSFCHUNK");
        clip_bytes.extend_from_slice(&[0_u8; 48]);
        clip_bytes.extend_from_slice(b"CHNKHead");
        clip_bytes.extend_from_slice(&[0_u8; 64]);
        clip_bytes.extend_from_slice(b"CHNKSQLi");
        clip_bytes.extend_from_slice(&(database_bytes.len() as u64).to_be_bytes());
        clip_bytes.extend_from_slice(database_bytes);
        clip_bytes.extend_from_slice(b"CHNKFoot");
        clip_bytes.extend_from_slice(&0_u64.to_be_bytes());
        clip_bytes
    }

    fn wrap_database_in_legacy_clip_container(database_bytes: &[u8]) -> Vec<u8> {
        let mut clip_bytes = Vec::with_capacity(128 + database_bytes.len());
        clip_bytes.extend_from_slice(b"CSFCHUNK");
        clip_bytes.extend_from_slice(&[0_u8; 48]);
        clip_bytes.extend_from_slice(b"CHNKHead");
        clip_bytes.extend_from_slice(&[0_u8; 64]);
        clip_bytes.extend_from_slice(database_bytes);
        clip_bytes
    }

    fn append_false_positive_chunk_header(clip_bytes: &mut Vec<u8>) {
        clip_bytes.extend_from_slice(b"CHNKSQLi");
        clip_bytes.extend_from_slice(&u64::MAX.to_be_bytes());
    }

    fn append_false_positive_sqlite_header(clip_bytes: &mut Vec<u8>) {
        clip_bytes.extend_from_slice(b"SQLite format 3\0");
        clip_bytes.extend_from_slice(&[0_u8; 84]);
    }
}
