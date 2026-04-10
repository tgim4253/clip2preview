# clip2preview

Rust crate for extracting preview images from `.clip` files.

What it does:

- finds the embedded SQLite database inside the `.clip` container
- reads `CanvasPreview`
- returns the largest preview row
- detects PNG, JPEG, or WebP from the blob bytes

## Rust

```rust
use clip2preview::extract_preview;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let preview = extract_preview("sample.clip")?;
    preview.save("preview.png")?;
    Ok(())
}
```

## API

| Function | Parameters | Returns |
| --- | --- | --- |
| `extract_preview(path)` | `path: P where P: AsRef<Path>` | `Result<Preview>` |
| `extract_preview_from_reader(reader)` | `reader: &mut R where R: Read + Seek` | `Result<Preview>` |

| `Preview` method | Parameters | Description |
| --- | --- | --- |
| `format()` | none | Returns `PreviewFormat` |
| `bytes()` | none | Returns encoded image bytes |
| `len()` | none | Returns byte length |
| `is_empty()` | none | Returns whether the payload is empty |
| `dimensions()` | none | Returns `(width, height)` if present |
| `save(path)` | `path: P where P: AsRef<Path>` | Writes the preview to disk |

## CLI

```bash
cargo run -- sample.clip
cargo run -- sample.clip preview.png
```

If no output path is given, it writes `sample.preview.<ext>` next to the input file.

| Argument | Required | Description |
| --- | --- | --- |
| `input.clip` | yes | Source `.clip` file |
| `output` | no | Output path for the extracted preview |

## Errors

| Error | Meaning |
| --- | --- |
| `ClipError::Io` | File open, read, seek, or write failed |
| `ClipError::Sqlite` | Embedded SQLite open or query failed |
| `ClipError::InvalidFormat` | Input is not a supported `.clip` layout |
| `ClipError::PreviewNotFound` | `CanvasPreview` exists but no usable preview row was found |
| `ClipError::Unsupported` | File looks related but uses a variation not handled yet |
