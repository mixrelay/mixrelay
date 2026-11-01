//! Checked-in JSON replies that the control UI's decoders are tested against.
//!
//! Each Rust owner serializes a representative reply into `tests/fixtures/control-replies`.
//! A test that finds its fixture stale rewrites it and fails, so rerunning the owning tests
//! (`pnpm fixtures:replies`) regenerates every fixture for review and commit.
use std::{fs, io, path::PathBuf};

use serde::Serialize;

/// Writes `value` as the fixture `name`; returns whether the file held anything else before,
/// including when it did not exist.
/// # Errors
/// Reports serialization and write failures.
pub fn refresh_reply_fixture(name: &str, value: &impl Serialize) -> io::Result<bool> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/control-replies")
        .join(format!("{name}.json"));
    let previous = fs::read_to_string(&path).unwrap_or_default();
    let current = serde_json::to_string_pretty(value)? + "\n";
    fs::write(&path, &current)?;
    Ok(previous != current)
}
