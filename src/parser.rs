use std::io::{Read, Seek, SeekFrom};

use rusqlite::{Connection, MAIN_DB, OptionalExtension};

use crate::error::{ClipError, Result};
use crate::preview::{Preview, PreviewFormat};

const CLIP_SIGNATURE: &[u8; 8] = b"CSFCHUNK";
const SQLITE_CHUNK_TAG: &[u8; 8] = b"CHNKSQLi";
const FOOT_CHUNK_TAG: &[u8; 8] = b"CHNKFoot";
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const SQLITE_DATABASE_HEADER_SIZE: usize = 100;
const SCAN_BUFFER_SIZE: usize = 64 * 1024;

pub(crate) struct ClipParser<'a, R> {
    reader: &'a mut R,
}

impl<'a, R: Read + Seek> ClipParser<'a, R> {
    pub(crate) fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }

    pub(crate) fn extract_preview(&mut self) -> Result<Preview> {
        self.ensure_clip_signature()?;
        let sqlite_chunk = self.find_embedded_sqlite_chunk()?;
        let connection = self.open_embedded_database(sqlite_chunk)?;
        Self::load_preview_from_connection(&connection)
    }

    fn ensure_clip_signature(&mut self) -> Result<()> {
        self.reader.seek(SeekFrom::Start(0))?;

        let mut signature = [0_u8; CLIP_SIGNATURE.len()];
        let read = self.reader.read(&mut signature)?;

        self.reader.seek(SeekFrom::Start(0))?;

        if read == 0 {
            return Err(ClipError::InvalidFormat("input is empty"));
        }

        if read < CLIP_SIGNATURE.len() || &signature != CLIP_SIGNATURE {
            return Err(ClipError::InvalidFormat("missing CSFCHUNK signature"));
        }

        Ok(())
    }

    fn find_embedded_sqlite_chunk(&mut self) -> Result<ChunkRange> {
        // Prefer the chunk header when present so we get an exact SQLite payload range.
        if let Some(chunk) = self.find_sqlite_chunk_by_chunk_header()? {
            return Ok(chunk);
        }

        // Older or less understood variants may still require locating SQLite by its file header.
        let offset = self.find_sqlite_chunk_by_sqlite_header()?;
        let file_len = self.reader.seek(SeekFrom::End(0))?;
        let size = file_len
            .checked_sub(offset)
            .ok_or(ClipError::InvalidFormat(
                "embedded SQLite offset is out of range",
            ))?;

        Ok(ChunkRange { offset, size })
    }

    fn find_sqlite_chunk_by_chunk_header(&mut self) -> Result<Option<ChunkRange>> {
        let file_len = self.reader.seek(SeekFrom::End(0))?;
        let mut position = file_len;
        let mut carry = Vec::new();

        while position > 0 {
            let start = position.saturating_sub(SCAN_BUFFER_SIZE as u64);
            let read_len = (position - start) as usize;
            let mut chunk = vec![0_u8; read_len];

            self.reader.seek(SeekFrom::Start(start))?;
            self.reader.read_exact(&mut chunk)?;

            let mut search: Vec<u8> = Vec::with_capacity(chunk.len() + carry.len());
            search.extend_from_slice(&chunk);
            search.extend_from_slice(&carry);

            let mut search_end = chunk.len();
            // Try every candidate in this window because later matches may be payload false positives.
            while let Some(index) =
                find_last_subslice_starting_before(&search, SQLITE_CHUNK_TAG, search_end)
            {
                let header_offset = start + index as u64;
                if let Some(chunk) = self.parse_sqlite_chunk_header(header_offset, file_len)? {
                    return Ok(Some(chunk));
                }

                search_end = index;
            }

            let keep = SQLITE_CHUNK_TAG.len().saturating_sub(1);
            // Preserve the leading edge so a split signature is still visible in the next window.
            carry.clear();
            carry.extend_from_slice(&chunk[..chunk.len().min(keep)]);
            position = start;
        }

        Ok(None)
    }

    fn find_sqlite_chunk_by_sqlite_header(&mut self) -> Result<u64> {
        let file_len = self.reader.seek(SeekFrom::End(0))?;
        let mut position = file_len;
        let mut carry = Vec::new();

        while position > 0 {
            let start = position.saturating_sub(SCAN_BUFFER_SIZE as u64);
            let read_len = (position - start) as usize;
            let mut chunk = vec![0_u8; read_len];

            self.reader.seek(SeekFrom::Start(start))?;
            self.reader.read_exact(&mut chunk)?;

            let mut search = Vec::with_capacity(chunk.len() + carry.len());
            search.extend_from_slice(&chunk);
            search.extend_from_slice(&carry);

            let mut search_end = chunk.len();
            // Try every candidate in this window because the raw SQLite header can also appear in payload data.
            while let Some(index) =
                find_last_subslice_starting_before(&search, SQLITE_HEADER, search_end)
            {
                let header_offset = start + index as u64;
                if self.is_valid_sqlite_header(header_offset, file_len)? {
                    return Ok(header_offset);
                }

                search_end = index;
            }

            let keep = SQLITE_HEADER.len().saturating_sub(1);
            // Preserve the leading edge so a split signature is still visible in the next window.
            carry.clear();
            carry.extend_from_slice(&chunk[..chunk.len().min(keep)]);
            position = start;
        }

        Err(ClipError::InvalidFormat("embedded SQLite header not found"))
    }

    fn parse_sqlite_chunk_header(
        &mut self,
        header_offset: u64,
        file_len: u64,
    ) -> Result<Option<ChunkRange>> {
        let mut header = [0_u8; 16];
        self.reader.seek(SeekFrom::Start(header_offset))?;
        self.reader.read_exact(&mut header)?;

        if &header[..8] != SQLITE_CHUNK_TAG {
            return Ok(None);
        }

        let chunk_size = u64::from_be_bytes(header[8..16].try_into().unwrap());
        let payload_offset = header_offset + header.len() as u64;
        let payload_end = match payload_offset.checked_add(chunk_size) {
            Some(payload_end) => payload_end,
            None => return Ok(None),
        };

        if payload_end > file_len {
            return Ok(None);
        }

        if payload_end + 8 <= file_len {
            let mut footer = [0_u8; 8];
            self.reader.seek(SeekFrom::Start(payload_end))?;
            self.reader.read_exact(&mut footer)?;

            if &footer != FOOT_CHUNK_TAG {
                return Ok(None);
            }
        }

        Ok(Some(ChunkRange {
            offset: payload_offset,
            size: chunk_size,
        }))
    }

    fn is_valid_sqlite_header(&mut self, header_offset: u64, file_len: u64) -> Result<bool> {
        let header_end = match header_offset.checked_add(SQLITE_DATABASE_HEADER_SIZE as u64) {
            Some(header_end) => header_end,
            None => return Ok(false),
        };

        if header_end > file_len {
            return Ok(false);
        }

        let mut header = [0_u8; SQLITE_DATABASE_HEADER_SIZE];
        self.reader.seek(SeekFrom::Start(header_offset))?;
        self.reader.read_exact(&mut header)?;

        if &header[..SQLITE_HEADER.len()] != SQLITE_HEADER {
            return Ok(false);
        }

        let page_size = u16::from_be_bytes([header[16], header[17]]);
        let write_version = header[18];
        let read_version = header[19];
        let fractions_are_valid = header[21] == 64 && header[22] == 32 && header[23] == 32;

        // These header fields are stable enough to reject accidental `SQLite format 3` matches.
        Ok(is_valid_sqlite_page_size(page_size)
            && matches!(write_version, 1 | 2)
            && matches!(read_version, 1 | 2)
            && fractions_are_valid)
    }

    fn open_embedded_database(&mut self, sqlite_chunk: ChunkRange) -> Result<Connection> {
        self.reader.seek(SeekFrom::Start(sqlite_chunk.offset))?;
        let mut limited_reader = self.reader.by_ref().take(sqlite_chunk.size);
        let mut connection = Connection::open_in_memory()?;
        // Deserialize straight into SQLite memory to avoid temp-file write/read churn.
        connection.deserialize_read_exact(
            MAIN_DB,
            &mut limited_reader,
            usize::try_from(sqlite_chunk.size)
                .map_err(|_| ClipError::Unsupported("SQLite chunk is too large to deserialize"))?,
            true,
        )?;
        Ok(connection)
    }

    fn load_preview_from_connection(connection: &Connection) -> Result<Preview> {
        let has_preview_table: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM sqlite_master
                WHERE type = 'table' AND name = 'CanvasPreview'
            )",
            [],
            |row| row.get(0),
        )?;

        if !has_preview_table {
            return Err(ClipError::InvalidFormat(
                "CanvasPreview table not found in embedded SQLite database",
            ));
        }

        let preview_row = connection
            .query_row(
                "SELECT ImageWidth, ImageHeight, ImageData
                 FROM CanvasPreview
                 WHERE ImageData IS NOT NULL AND length(ImageData) > 0
                 ORDER BY (COALESCE(ImageWidth, 0) * COALESCE(ImageHeight, 0)) DESC,
                          _PW_ID ASC
                 LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(ClipError::PreviewNotFound)?;

        let (width, height, bytes) = preview_row;
        if bytes.is_empty() {
            return Err(ClipError::PreviewNotFound);
        }

        let mut preview = Preview::new(PreviewFormat::detect(&bytes), bytes);

        if let (Some(width), Some(height)) = (to_dimension(width), to_dimension(height)) {
            preview = preview.with_dimensions(width, height);
        }

        Ok(preview)
    }
}

#[derive(Debug, Clone, Copy)]
struct ChunkRange {
    offset: u64,
    size: u64,
}

fn find_last_subslice_starting_before(
    haystack: &[u8],
    needle: &[u8],
    exclusive_end: usize,
) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    let limit = exclusive_end.min(haystack.len());
    if haystack.len() < needle.len() || limit == 0 {
        return None;
    }

    let last_start = limit - 1;
    (0..=last_start)
        .rev()
        .find(|&index| haystack.get(index..index + needle.len()) == Some(needle))
}

fn to_dimension(value: Option<i64>) -> Option<u32> {
    let value = value?;
    if value <= 0 {
        return None;
    }

    u32::try_from(value).ok()
}

fn is_valid_sqlite_page_size(page_size: u16) -> bool {
    page_size == 1 || ((512..=32_768).contains(&page_size) && page_size.is_power_of_two())
}
