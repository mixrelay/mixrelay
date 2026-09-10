//! Linux process identity is PID plus kernel start time, never a display name.
use std::{fs, io::Read, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub process_id: u32,
    pub parent_id: u32,
    pub start_time: u64,
    pub executable_path: Option<String>,
}

/// Read-only, bounded `/proc` lookup. Recheck the birth stamp around readlink so
/// process exit/PID reuse cannot combine two processes' identity fields.
#[must_use]
pub fn process_identity(process_id: u32) -> Option<ProcessIdentity> {
    let (parent_id, start_time) = process_stamp(process_id)?;
    let executable_path = fs::read_link(format!("/proc/{process_id}/exe"))
        .ok()
        .filter(|path| path.is_absolute())
        .and_then(|path| {
            path.to_str()
                .filter(|value| value.len() <= 4096 && !value.ends_with(" (deleted)"))
                .map(str::to_owned)
        });
    if process_stamp(process_id)? != (parent_id, start_time) {
        return None;
    }
    Some(ProcessIdentity {
        process_id,
        parent_id,
        start_time,
        executable_path,
    })
}

fn process_stamp(process_id: u32) -> Option<(u32, u64)> {
    let mut bytes = Vec::new();
    fs::File::open(format!("/proc/{process_id}/stat"))
        .ok()?
        .take(4097)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    parse_stat(std::str::from_utf8(&bytes).ok()?)
}

fn parse_stat(stat: &str) -> Option<(u32, u64)> {
    let fields = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    Some((fields.get(1)?.parse().ok()?, fields.get(19)?.parse().ok()?))
}

#[must_use]
pub fn same_process(left: &ProcessIdentity, right: &ProcessIdentity) -> bool {
    left.process_id == right.process_id && left.start_time == right.start_time
}

/// A namespace-local or forged `PipeWire` PID must not turn an unrelated host
/// executable into persistent application intent.
#[must_use]
pub fn verified_executable(process_id: u32, reported_binary: Option<&str>) -> Option<String> {
    let path = process_identity(process_id)?.executable_path?;
    let reported = reported_binary?;
    let matches = if Path::new(reported).is_absolute() {
        path == reported
    } else {
        Path::new(&path).file_name().and_then(|name| name.to_str()) == Some(reported)
    };
    matches.then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stat_parser_handles_spaces_and_parentheses_and_rejects_truncation() {
        let line = format!(
            "123 (a ) strange name) S 42 {} 987",
            vec!["0"; 17].join(" ")
        );
        assert_eq!(parse_stat(&line), Some((42, 987)));
        assert!(parse_stat("123 (x) S 42").is_none());
    }
    #[test]
    fn current_process_read_is_stable_and_wrong_binary_is_not_verified() {
        let pid = std::process::id();
        let identity = process_identity(pid).expect("own proc identity");
        assert!(same_process(&identity, &process_identity(pid).unwrap()));
        assert!(verified_executable(pid, Some("definitely-not-mixrelay")).is_none());
        let executable = identity.executable_path.unwrap();
        assert_eq!(
            verified_executable(pid, Some(&executable)),
            Some(executable)
        );
    }
}
