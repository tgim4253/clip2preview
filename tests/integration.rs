use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clip2preview::{ClipError, PreviewFormat, extract_preview};

#[test]
fn returns_io_error_for_missing_file() {
    let path = unique_test_path("missing.clip");
    let error = extract_preview(&path).unwrap_err();

    assert!(matches!(error, ClipError::Io(_)));
}

#[test]
fn returns_invalid_format_for_non_clip_file() {
    let path = unique_test_path("not-a-clip.bin");
    fs::write(&path, b"not a clip").unwrap();

    let error = extract_preview(&path).unwrap_err();
    assert!(matches!(
        error,
        ClipError::InvalidFormat("missing CSFCHUNK signature")
    ));

    fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires CLIP2PREVIEW_SAMPLE_PATH to point at a local .clip file"]
fn extracts_preview_from_real_clip_sample() {
    let path = PathBuf::from(
        std::env::var_os("CLIP2PREVIEW_SAMPLE_PATH")
            .expect("CLIP2PREVIEW_SAMPLE_PATH environment variable must be set"),
    );

    let preview = extract_preview(&path).unwrap();
    assert!(matches!(
        preview.format(),
        PreviewFormat::Png | PreviewFormat::Jpeg | PreviewFormat::Webp
    ));
    assert!(!preview.is_empty(), "{path:?}");
    assert!(preview.dimensions().is_some(), "{path:?}");
}

fn unique_test_path(file_name: &str) -> PathBuf {
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    std::env::temp_dir().join(format!("{timestamp_nanos}-{file_name}"))
}
