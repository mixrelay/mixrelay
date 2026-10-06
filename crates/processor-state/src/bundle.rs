//! Portable, path-free framing for validated processor-state blobs.

use std::io::{self, Read, Write};

use crate::{StateError, StateRef, StateStore, store::verify_payload_stream};

const MAGIC: &[u8; 8] = b"MRPSTBND";
const VERSION: u16 = 1;

/// Maximum serialized manifest size accepted by a processor-state bundle.
pub const MAX_BUNDLE_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
/// Maximum number of state references in a processor-state bundle.
pub const MAX_BUNDLE_REFERENCES: usize = 4096;
/// Maximum combined state payload size in a processor-state bundle.
pub const MAX_BUNDLE_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// Writes a portable bundle containing a manifest and sorted, unique state payloads.
///
/// The framing uses a fixed magic, version, bounded little-endian lengths, raw SHA-256
/// digests, and payload bytes; it contains no filesystem paths or extraction instructions.
/// Payloads are verified before any archive bytes are written, then streamed and verified
/// again while copying.
///
/// # Arguments
/// * `writer` - Destination for the binary bundle.
/// * `manifest` - Manifest bytes, limited to [`MAX_BUNDLE_MANIFEST_BYTES`].
/// * `references` - Strictly digest-sorted, unique state references.
/// * `store` - Store which owns every referenced payload.
///
/// # Returns
/// Returns `Ok(())` after the complete archive has been written.
///
/// # Errors
/// Returns [`StateError`] for invalid manifest or reference bounds, duplicate or unsorted
/// references, missing/corrupt store blobs, or reader/writer failures.
pub fn write_bundle(
    mut writer: impl Write,
    manifest: &[u8],
    references: &[StateRef],
    store: &StateStore,
) -> Result<(), StateError> {
    let total_payload = validate_manifest_and_references(manifest, references)?;
    if total_payload > MAX_BUNDLE_PAYLOAD_BYTES {
        return Err(StateError::InvalidBundle {
            reason: "combined payload exceeds the maximum bundle size",
        });
    }
    for reference in references {
        store.verify(reference)?;
    }
    let manifest_length = u32::try_from(manifest.len()).map_err(|_| StateError::InvalidBundle {
        reason: "manifest length does not fit bundle framing",
    })?;
    let reference_count =
        u32::try_from(references.len()).map_err(|_| StateError::InvalidBundle {
            reason: "reference count does not fit bundle framing",
        })?;

    writer.write_all(MAGIC)?;
    writer.write_all(&VERSION.to_le_bytes())?;
    writer.write_all(&manifest_length.to_le_bytes())?;
    writer.write_all(&reference_count.to_le_bytes())?;
    writer.write_all(manifest)?;
    for reference in references {
        writer.write_all(&digest_bytes(reference)?)?;
        writer.write_all(&reference.byte_length().to_le_bytes())?;
        store.copy_verified(reference, &mut writer)?;
    }
    Ok(())
}

/// Reads a bounded bundle and optionally interns each verified payload in `store`.
///
/// Preview reads pass `None`: every payload is streamed through SHA-256 without writing it to
/// disk. With `Some`, blobs are immutably interned as soon as each payload verifies; those
/// harmless blobs can remain if a later frame or trailing-byte check fails.
///
/// # Arguments
/// * `reader` - Source of the complete versioned bundle.
/// * `store` - Optional destination store; `None` verifies without persisting payloads.
///
/// # Returns
/// The manifest bytes and the exact ordered state references embedded in the bundle.
///
/// # Errors
/// Returns [`StateError`] for unsupported framing, oversized fields, duplicate or unsorted
/// references, truncated/corrupt payloads, trailing bytes, or reader/store failures.
pub fn read_bundle(
    mut reader: impl Read,
    store: Option<&StateStore>,
) -> Result<(Vec<u8>, Vec<StateRef>), StateError> {
    let mut magic = [0_u8; 8];
    read_framing(&mut reader, &mut magic)?;
    if &magic != MAGIC {
        return Err(StateError::InvalidBundle {
            reason: "unrecognized bundle magic",
        });
    }
    let version = read_u16(&mut reader)?;
    if version != VERSION {
        return Err(StateError::InvalidBundle {
            reason: "unsupported bundle version",
        });
    }
    let manifest_length = read_u32(&mut reader)? as usize;
    let reference_count = read_u32(&mut reader)? as usize;
    if manifest_length > MAX_BUNDLE_MANIFEST_BYTES {
        return Err(StateError::InvalidBundle {
            reason: "manifest exceeds the maximum byte length",
        });
    }
    if reference_count > MAX_BUNDLE_REFERENCES {
        return Err(StateError::InvalidBundle {
            reason: "reference count exceeds the maximum",
        });
    }

    let mut manifest = Vec::new();
    manifest
        .try_reserve_exact(manifest_length)
        .map_err(|_| StateError::Allocation)?;
    manifest.resize(manifest_length, 0);
    read_framing(&mut reader, &mut manifest)?;

    let mut references = Vec::new();
    references
        .try_reserve_exact(reference_count)
        .map_err(|_| StateError::Allocation)?;
    let mut previous_digest: Option<String> = None;
    let mut total_payload = 0_u64;
    for _ in 0..reference_count {
        let mut digest = [0_u8; 32];
        read_framing(&mut reader, &mut digest)?;
        let byte_length = read_u64(&mut reader)?;
        let reference = StateRef::new(hex_digest(&digest), byte_length)?;
        require_next_digest(previous_digest.as_deref(), &reference)?;
        previous_digest = Some(reference.sha256().to_owned());
        total_payload = total_payload
            .checked_add(byte_length)
            .filter(|total| *total <= MAX_BUNDLE_PAYLOAD_BYTES)
            .ok_or(StateError::InvalidBundle {
                reason: "combined payload exceeds the maximum bundle size",
            })?;

        if let Some(store) = store {
            store.store_payload(&mut reader, byte_length, Some(&reference))?;
        } else {
            verify_payload_stream(&mut reader, &reference)?;
        }
        references.push(reference);
    }
    let mut trailing = [0_u8; 1];
    if read_retry(&mut reader, &mut trailing)? != 0 {
        return Err(StateError::InvalidBundle {
            reason: "trailing bytes after the final payload",
        });
    }
    Ok((manifest, references))
}

fn validate_manifest_and_references(
    manifest: &[u8],
    references: &[StateRef],
) -> Result<u64, StateError> {
    if manifest.len() > MAX_BUNDLE_MANIFEST_BYTES {
        return Err(StateError::InvalidBundle {
            reason: "manifest exceeds the maximum byte length",
        });
    }
    if references.len() > MAX_BUNDLE_REFERENCES {
        return Err(StateError::InvalidBundle {
            reason: "reference count exceeds the maximum",
        });
    }
    let mut previous_digest: Option<&str> = None;
    let mut total_payload = 0_u64;
    for reference in references {
        reference.validate()?;
        require_next_digest(previous_digest, reference)?;
        previous_digest = Some(reference.sha256());
        total_payload = total_payload
            .checked_add(reference.byte_length())
            .filter(|total| *total <= MAX_BUNDLE_PAYLOAD_BYTES)
            .ok_or(StateError::InvalidBundle {
                reason: "combined payload exceeds the maximum bundle size",
            })?;
    }
    Ok(total_payload)
}

fn require_next_digest(previous: Option<&str>, next: &StateRef) -> Result<(), StateError> {
    if previous.is_some_and(|previous| previous >= next.sha256()) {
        return Err(StateError::InvalidBundle {
            reason: "references must be strictly sorted by digest and contain no duplicates",
        });
    }
    Ok(())
}

fn digest_bytes(reference: &StateRef) -> Result<[u8; 32], StateError> {
    reference.validate()?;
    let mut bytes = [0_u8; 32];
    for (index, pair) in reference.sha256().as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
    }
    Ok(bytes)
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(HEX[usize::from(*byte >> 4)] as char);
        encoded.push(HEX[usize::from(*byte & 0x0f)] as char);
    }
    encoded
}

fn hex_value(byte: u8) -> Result<u8, StateError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(StateError::InvalidReference {
            reason: "digest must be 64 lowercase hexadecimal characters",
        }),
    }
}

fn read_u16(reader: &mut impl Read) -> Result<u16, StateError> {
    let mut bytes = [0_u8; 2];
    read_framing(reader, &mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> Result<u32, StateError> {
    let mut bytes = [0_u8; 4];
    read_framing(reader, &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64, StateError> {
    let mut bytes = [0_u8; 8];
    read_framing(reader, &mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_framing(reader: &mut impl Read, bytes: &mut [u8]) -> Result<(), StateError> {
    match reader.read_exact(bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            Err(StateError::InvalidBundle {
                reason: "truncated bundle framing",
            })
        }
        Err(error) => Err(StateError::Io(error)),
    }
}

fn read_retry(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use mixrelay_test_support::FixtureDir;

    use super::*;

    fn test_store(name: &str) -> (FixtureDir, StateStore) {
        let fixture = FixtureDir::new("processor-state-bundle")
            .expect("create checkout-local fixture directory");
        let store = StateStore::new(fixture.join(name));
        (fixture, store)
    }

    fn raw_bundle(manifest: &[u8], frames: &[(&StateRef, &[u8])]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        let manifest_length = u32::try_from(manifest.len()).expect("test manifest fits framing");
        let frame_count = u32::try_from(frames.len()).expect("test frame count fits framing");
        bytes.extend_from_slice(&manifest_length.to_le_bytes());
        bytes.extend_from_slice(&frame_count.to_le_bytes());
        bytes.extend_from_slice(manifest);
        for (reference, payload) in frames {
            bytes.extend_from_slice(&digest_bytes(reference).unwrap());
            bytes.extend_from_slice(&reference.byte_length().to_le_bytes());
            bytes.extend_from_slice(payload);
        }
        bytes
    }

    #[test]
    fn portable_round_trip_verifies_and_interns_in_another_store() {
        let (_source_dir, source) = test_store("source");
        let (_target_dir, target) = test_store("target");
        let mut entries = [source.put(b"one").unwrap(), source.put(b"two").unwrap()];
        entries.sort_by(|left, right| left.sha256().cmp(right.sha256()));

        let mut archive = Vec::new();
        write_bundle(&mut archive, b"manifest", &entries, &source).unwrap();
        let (manifest, references) = read_bundle(Cursor::new(&archive), Some(&target)).unwrap();

        assert_eq!(manifest, b"manifest");
        assert_eq!(references, entries);
        for reference in &references {
            target.verify(reference).unwrap();
        }
        let path_bytes = source.root().as_os_str().as_encoded_bytes();
        assert!(
            !archive
                .windows(path_bytes.len())
                .any(|window| window == path_bytes)
        );
    }

    #[test]
    fn preview_verifies_payloads_without_creating_a_store() {
        let (_source_dir, source) = test_store("source");
        let (_preview_dir, preview) = test_store("preview");
        let reference = source.put(b"portable").unwrap();
        let mut archive = Vec::new();
        write_bundle(
            &mut archive,
            b"m",
            std::slice::from_ref(&reference),
            &source,
        )
        .unwrap();

        let (manifest, references) = read_bundle(Cursor::new(archive), None).unwrap();

        assert_eq!(manifest, b"m");
        assert_eq!(references, [reference]);
        assert!(!preview.root().exists());
    }

    #[test]
    fn writer_rejects_duplicate_and_unsorted_references_before_writing() {
        let (_temporary, store) = test_store("state");
        let first = store.put(b"first").unwrap();
        let second = store.put(b"second").unwrap();
        let (lower, higher) = if first.sha256() < second.sha256() {
            (first, second)
        } else {
            (second, first)
        };
        let mut output = Vec::new();
        assert!(write_bundle(&mut output, b"m", &[lower.clone(), lower.clone()], &store).is_err());
        assert!(output.is_empty());
        assert!(write_bundle(&mut output, b"m", &[higher, lower], &store).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn reader_rejects_duplicate_references_truncation_corruption_and_trailing_data() {
        let reference = StateRef::new(
            "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
            1,
        )
        .unwrap();
        let duplicate = raw_bundle(b"m", &[(&reference, b"x"), (&reference, b"x")]);
        assert!(read_bundle(Cursor::new(duplicate), None).is_err());

        let valid = raw_bundle(b"m", &[(&reference, b"x")]);
        assert!(read_bundle(Cursor::new(&valid[..valid.len() - 1]), None).is_err());
        let mut corrupted = valid.clone();
        *corrupted.last_mut().unwrap() = b'y';
        assert!(read_bundle(Cursor::new(corrupted), None).is_err());
        let mut trailing = valid;
        trailing.push(0);
        assert!(read_bundle(Cursor::new(trailing), None).is_err());
    }

    #[test]
    fn reader_rejects_oversized_state_references_before_payload() {
        let mut oversized = Vec::new();
        oversized.extend_from_slice(MAGIC);
        oversized.extend_from_slice(&VERSION.to_le_bytes());
        oversized.extend_from_slice(&0_u32.to_le_bytes());
        oversized.extend_from_slice(&1_u32.to_le_bytes());
        oversized.extend_from_slice(&[0_u8; 32]);
        oversized.extend_from_slice(&((crate::MAX_STATE_BYTES as u64) + 1).to_le_bytes());
        assert!(read_bundle(Cursor::new(oversized), None).is_err());
    }

    #[test]
    fn bundle_limits_reject_oversized_manifest_count_and_combined_payload() {
        let mut manifest = Vec::new();
        manifest.extend_from_slice(MAGIC);
        manifest.extend_from_slice(&VERSION.to_le_bytes());
        let oversized_manifest =
            u32::try_from(MAX_BUNDLE_MANIFEST_BYTES + 1).expect("test bound fits framing");
        manifest.extend_from_slice(&oversized_manifest.to_le_bytes());
        manifest.extend_from_slice(&0_u32.to_le_bytes());
        assert!(read_bundle(Cursor::new(manifest), None).is_err());

        let mut count = Vec::new();
        count.extend_from_slice(MAGIC);
        count.extend_from_slice(&VERSION.to_le_bytes());
        count.extend_from_slice(&0_u32.to_le_bytes());
        let oversized_count =
            u32::try_from(MAX_BUNDLE_REFERENCES + 1).expect("test bound fits framing");
        count.extend_from_slice(&oversized_count.to_le_bytes());
        assert!(read_bundle(Cursor::new(count), None).is_err());

        let (_temporary, store) = test_store("state");
        let references = (1..=17)
            .map(|digest| {
                StateRef::new(format!("{digest:064x}"), crate::MAX_STATE_BYTES as u64).unwrap()
            })
            .collect::<Vec<_>>();
        let mut output = Vec::new();
        assert!(write_bundle(&mut output, b"m", &references, &store).is_err());
        assert!(output.is_empty());
    }

    #[test]
    fn bad_later_frame_leaves_only_already_verified_immutable_blobs() {
        let (_source_dir, source) = test_store("source");
        let (_target_dir, target) = test_store("target");
        let first = source.put(b"valid").unwrap();
        let second = source.put(b"invalid").unwrap();
        let mut frames = [
            (first.clone(), b"valid".as_slice()),
            (second.clone(), b"invalid".as_slice()),
        ];
        frames.sort_by(|left, right| left.0.sha256().cmp(right.0.sha256()));
        let mut archive = raw_bundle(
            b"m",
            &frames
                .iter()
                .map(|(reference, payload)| (reference, *payload))
                .collect::<Vec<_>>(),
        );
        let last = archive.len() - 1;
        archive[last] ^= 1;

        assert!(read_bundle(Cursor::new(archive), Some(&target)).is_err());
        assert!(target.verify(&frames[0].0).is_ok());
        assert!(target.verify(&frames[1].0).is_err());
    }
}
