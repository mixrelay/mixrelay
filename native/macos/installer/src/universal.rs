//! Bounded Universal 2 header inspection without requiring Xcode or `lipo`.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
};

use thiserror::Error;

const X86_64: u32 = 0x0100_0007;
const ARM64: u32 = 0x0100_000c;

/// A rejected Universal 2 executable or failed binary read.
#[derive(Debug, Error)]
pub enum UniversalError {
    /// The wrapper is not exactly one arm64 and one `x86_64` slice.
    #[error("expected a Universal 2 Mach-O with one arm64 and one x86_64 slice")]
    Architectures,
    /// Slice bounds, alignment or inner Mach-O identity are invalid.
    #[error("invalid Universal 2 slice bounds or Mach-O header")]
    Structure,
    /// The executable could not be read.
    #[error("could not inspect Universal 2 executable: {0}")]
    Io(#[from] io::Error),
}

/// Verifies the two architecture headers and their disjoint in-file ranges.
///
/// Reads a fixed-size table and two inner headers, not the whole executable.
/// This is a format check, not a signature check or path-ownership check.
/// # Errors
/// Returns missing/extra architectures, malformed ranges/headers, or read failure.
pub fn validate_universal_binary(path: &Path) -> Result<(), UniversalError> {
    let mut file = open_binary(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(UniversalError::Structure);
    }
    let length = metadata.len();
    validate(&mut file, length)
}

#[cfg(unix)]
fn open_binary(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags};
    Ok(File::from(rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}

#[cfg(not(unix))]
fn open_binary(path: &Path) -> io::Result<File> {
    File::open(path)
}

#[derive(Clone, Copy)]
struct Slice {
    cpu: u32,
    offset: u64,
    end: u64,
}

fn validate(reader: &mut (impl Read + Seek), length: u64) -> Result<(), UniversalError> {
    let mut header = [0; 8];
    reader.read_exact(&mut header)?;
    let entry_size = match u32::from_be_bytes(header[..4].try_into().expect("four-byte magic")) {
        0xcafe_babe => 20,
        0xcafe_babf => 32,
        _ => return Err(UniversalError::Architectures),
    };
    if u32::from_be_bytes(header[4..].try_into().expect("four-byte count")) != 2 {
        return Err(UniversalError::Architectures);
    }
    let first = read_slice(reader, entry_size, length)?;
    let second = read_slice(reader, entry_size, length)?;
    if !matches!((first.cpu, second.cpu), (ARM64, X86_64) | (X86_64, ARM64)) {
        return Err(UniversalError::Architectures);
    }
    if first.offset < second.end && second.offset < first.end {
        return Err(UniversalError::Structure);
    }
    for slice in [first, second] {
        reader.seek(SeekFrom::Start(slice.offset))?;
        reader.read_exact(&mut header)?;
        if u32::from_le_bytes(header[..4].try_into().expect("four-byte magic")) != 0xfeed_facf
            || u32::from_le_bytes(header[4..].try_into().expect("four-byte CPU")) != slice.cpu
        {
            return Err(UniversalError::Structure);
        }
    }
    Ok(())
}

fn read_slice(
    reader: &mut impl Read,
    entry_size: usize,
    length: u64,
) -> Result<Slice, UniversalError> {
    let mut bytes = [0; 32];
    reader.read_exact(&mut bytes[..entry_size])?;
    let word = |offset: usize| {
        u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("bounded word"))
    };
    let (offset, size, alignment) = if entry_size == 20 {
        (u64::from(word(8)), u64::from(word(12)), word(16))
    } else {
        if word(28) != 0 {
            return Err(UniversalError::Structure);
        }
        (
            u64::from_be_bytes(bytes[8..16].try_into().expect("eight-byte offset")),
            u64::from_be_bytes(bytes[16..24].try_into().expect("eight-byte size")),
            word(24),
        )
    };
    let end = offset.checked_add(size).ok_or(UniversalError::Structure)?;
    let minimum_offset = if entry_size == 20 { 48 } else { 72 };
    if size < 32
        || offset < minimum_offset
        || end > length
        || alignment >= 63
        || offset % (1_u64 << alignment) != 0
    {
        return Err(UniversalError::Structure);
    }
    Ok(Slice {
        cpu: word(0),
        offset,
        end,
    })
}

#[cfg(test)]
pub(crate) mod tests;
