use super::*;
use serde::{
    Serializer,
    ser::{Error, SerializeSeq},
};

fn fixture() -> mixrelay_test_support::FixtureDir {
    mixrelay_test_support::FixtureDir::new("document-persistence").unwrap()
}

#[test]
fn commit_replaces_only_after_explicit_acceptance_and_leaves_no_staged_file() {
    let directory = fixture();
    let path = directory.join("graphs.json");
    let accepted = b"{\"revision\":1}\n";
    fs::write(&path, accepted).unwrap();

    let staged = stage_json(&path, &serde_json::json!({"revision":2})).unwrap();
    assert_eq!(fs::read(&path).unwrap(), accepted);
    assert_eq!(staged.temporary.parent(), path.parent());
    staged.commit().unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"{\"revision\":2}\n");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn cancelling_does_not_replace_or_create_the_destination() {
    let directory = fixture();
    let path = directory.join("audio-session.json");
    drop(stage_json(&path, &17).unwrap());
    assert!(!path.exists());
    fs::write(&path, b"accepted").unwrap();

    drop(stage_json(&path, &18).unwrap());

    assert_eq!(fs::read(&path).unwrap(), b"accepted");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

struct PartialSerializationFailure;

impl Serialize for PartialSerializationFailure {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(2))?;
        sequence.serialize_element(&"partial data")?;
        Err(S::Error::custom("fixture serialization failure"))
    }
}

#[test]
fn partial_serialization_failure_cleans_staging_without_touching_accepted_bytes() {
    let directory = fixture();
    let path = directory.join("graphs.json");
    fs::write(&path, b"accepted").unwrap();

    assert!(stage_json(&path, &PartialSerializationFailure).is_err());

    assert_eq!(fs::read(&path).unwrap(), b"accepted");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn failed_commit_cleans_staging_and_preserves_the_destination() {
    let directory = fixture();
    let destination = directory.join("occupied-directory");
    fs::create_dir(&destination).unwrap();
    let marker = destination.join("keep");
    fs::write(&marker, b"accepted").unwrap();
    let staged = stage_json(&destination, &17).unwrap();
    let temporary = staged.temporary.clone();

    assert!(staged.commit().is_err());

    assert!(!temporary.exists());
    assert_eq!(fs::read(&marker).unwrap(), b"accepted");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn cancelling_one_transaction_does_not_remove_another_transactions_file() {
    let directory = fixture();
    let path = directory.join("graphs.json");
    let first = stage_json(&path, &1).unwrap();
    let second = stage_json(&path, &2).unwrap();
    assert_ne!(first.temporary, second.temporary);

    drop(first);

    assert!(second.temporary.exists());
    second.commit().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"2\n");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn newly_created_configuration_storage_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = fixture();
    let path = directory.join("private/graphs.json");
    let staged = stage_json(&path, &1).unwrap();
    let parent = path.parent().unwrap();

    assert_eq!(
        fs::metadata(parent).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&staged.temporary)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    staged.commit().unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
