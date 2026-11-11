//! Resource limits for untrusted media, applied before parsing or decoding.

use crate::errors::{Error, Result};
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use std::fs::OpenOptions;
use std::io::{self, Cursor, Read};
use std::path::Path;

// Large enough for common multi-face system collections and camera photographs.
pub(crate) const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) fn read_file(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    // O_NONBLOCK prevents a FIFO from blocking during open, before fstat can
    // reject it. O_NOCTTY avoids acquiring a terminal if a path is replaced.
    // Regular files ignore O_NONBLOCK; metadata below checks the actual handle.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input must be a regular file",
        ));
    }
    if metadata.len() > limit {
        return Err(too_large(limit));
    }
    // Metadata is only an early rejection. Read at most limit + 1 from this
    // same handle, so concurrent growth/replacement cannot bypass the ceiling.
    read_bounded(file, limit)
}

fn too_large(limit: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("input exceeds {limit} byte limit"),
    )
}

fn read_bounded(reader: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large(limit));
    }
    Ok(bytes)
}

pub(crate) fn open_image(path: &Path) -> Result<DynamicImage> {
    let bytes = read_file(path, MAX_IMAGE_BYTES)?;
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    decoder.set_limits(limits)?;
    let (width, height) = decoder.dimensions();
    // Check independently as well: allocation limits are advisory for some
    // codecs, and dimensions alone do not bound high-bit-depth pixel buffers.
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || decoder.total_bytes() > MAX_DECODE_BYTES
    {
        return Err(Error::msg(
            "image exceeds source dimension or decoded byte limits",
        ));
    }
    Ok(DynamicImage::from_decoder(decoder)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn fifo_without_writer_is_rejected_without_blocking_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.fifo");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        // Keep the test itself bounded if a future regression removes O_NONBLOCK.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            sender.send(read_file(&path, 4)).unwrap();
        });
        let result = receiver
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("opening a FIFO without a writer must not block");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn opened_file_limit_is_inclusive() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), b"1234").unwrap();
        assert_eq!(read_file(file.path(), 4).unwrap(), b"1234");
        assert!(read_file(file.path(), 3).is_err());
        assert!(read_file(file.path().parent().unwrap(), 4).is_err());
    }

    #[test]
    fn byte_limit_accepts_boundary_and_rejects_growth() {
        assert_eq!(read_bounded(Cursor::new(b"1234"), 4).unwrap(), b"1234");
        // A reader with no trustworthy size mirrors a file growing after stat.
        assert!(read_bounded(Cursor::new(b"12345"), 4).is_err());
        assert!(read_bounded(io::repeat(0), 4).is_err());
    }
}
