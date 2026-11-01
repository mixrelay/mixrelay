//! Checked-in JSON replies that the control UI's decoders are tested against.
//!
//! Each Rust owner serializes a representative reply into `tests/fixtures/control-replies`.
//! A test that finds its fixture stale rewrites it and fails, so rerunning the owning tests
//! (`pnpm fixtures:replies`) regenerates every fixture for review and commit.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;

/// Writes `value` as the fixture `name` when it differs; returns whether it differed,
/// including when the file did not exist. Unchanged fixtures are left untouched.
/// # Errors
/// Reports serialization and write failures.
pub fn refresh_reply_fixture(name: &str, value: &impl Serialize) -> io::Result<bool> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/control-replies")
        .join(format!("{name}.json"));
    refresh(&path, value)
}

fn refresh(path: &Path, value: &impl Serialize) -> io::Result<bool> {
    let previous = fs::read_to_string(path).unwrap_or_default();
    let current = serde_json::to_string_pretty(value)? + "\n";
    if previous == current {
        return Ok(false);
    }
    fs::write(path, &current)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FixtureDir;

    #[test]
    fn rewrites_only_a_missing_or_stale_fixture() {
        let directory = FixtureDir::new("reply-fixture").unwrap();
        let path = directory.join("reply.json");
        assert!(refresh(&path, &1).unwrap());
        let written = fs::metadata(&path).unwrap().modified().unwrap();
        assert!(!refresh(&path, &1).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), written);
        assert!(refresh(&path, &2).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "2\n");
    }
}
