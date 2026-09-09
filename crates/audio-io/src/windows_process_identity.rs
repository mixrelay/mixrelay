//! Executable intent and verified live process ownership. Never resolve by caption
//! or filename, and never use a snapshot PID without checking its creation time.
use std::{
    collections::BTreeMap,
    mem::size_of,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    time::SystemTime,
};
use windows::{
    Win32::{
        Foundation::{FILETIME, HANDLE, WAIT_TIMEOUT},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW,
                WaitForSingleObject,
            },
        },
    },
    core::PWSTR,
};

/// An owned process object, not merely a reusable numeric PID.
pub struct ProcessIdentity {
    handle: OwnedHandle,
    pid: u32,
    created: u64,
    path: String,
}
impl ProcessIdentity {
    /// # Errors
    /// Returns an error for exited, protected, or unresolvable processes.
    pub fn open(pid: u32) -> Result<Self, String> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map_err(|error| error.to_string())?;
        // SAFETY: successful OpenProcess transfers one owned kernel handle.
        let owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        unsafe {
            GetProcessTimes(
                handle,
                &raw mut created,
                &raw mut exited,
                &raw mut kernel,
                &raw mut user,
            )
        }
        .map_err(|error| error.to_string())?;
        let mut path = vec![0_u16; 32768];
        let mut length = 32_768_u32;
        unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &raw mut length,
            )
        }
        .map_err(|error| error.to_string())?;
        let length = usize::try_from(length).map_err(|error| error.to_string())?;
        let path = String::from_utf16(path.get(..length).ok_or("process path exceeds buffer")?)
            .map_err(|error| error.to_string())?;
        let path = canonical_path(&path)?;
        let result = Self {
            handle: owned,
            pid,
            created: u64::from(created.dwLowDateTime) | (u64::from(created.dwHighDateTime) << 32),
            path,
        };
        if !result.alive() {
            return Err("application process exited".into());
        }
        Ok(result)
    }
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.pid
    }
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    #[must_use]
    pub const fn creation_time(&self) -> u64 {
        self.created
    }
    #[must_use]
    pub fn alive(&self) -> bool {
        unsafe { WaitForSingleObject(HANDLE(self.handle.as_raw_handle()), 0) == WAIT_TIMEOUT }
    }
}

/// # Errors
/// Only an exact existing absolute executable path is resolved; no basename search.
pub fn canonical_path(path: &str) -> Result<String, String> {
    if !Path::new(path).is_absolute() {
        return Err("application executable path must be absolute".into());
    }
    std::fs::canonicalize(path)
        .map_err(|error| error.to_string())?
        .into_os_string()
        .into_string()
        .map_err(|_| "application path is not valid Unicode".into())
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolutionError {
    Unavailable,
    Ambiguous,
}

/// Resolve one live root for the exact executable, collapsing only verified
/// ancestors. Multiple unrelated instances remain explicitly ambiguous.
/// # Errors
/// Reports unavailable or ambiguous intent without selecting an arbitrary PID.
pub fn resolve_executable(path: &str) -> Result<ProcessIdentity, ResolutionError> {
    let path = canonical_path(path).map_err(|_| ResolutionError::Unavailable)?;
    let mut processes = snapshot().map_err(|_| ResolutionError::Unavailable)?;
    let rows = processes
        .iter()
        .filter(|(_, (_, identity))| identity.alive())
        .map(|(pid, (parent, identity))| {
            (
                *pid,
                (
                    *parent,
                    identity.created,
                    identity.path == path && identity.alive(),
                ),
            )
        })
        .collect();
    let roots = root_matches(&rows);
    if roots.len() > 1 {
        return Err(ResolutionError::Ambiguous);
    }
    roots
        .first()
        .and_then(|pid| processes.remove(pid))
        .map(|(_, identity)| identity)
        .filter(ProcessIdentity::alive)
        .ok_or(ResolutionError::Unavailable)
}

fn snapshot() -> Result<BTreeMap<u32, (u32, ProcessIdentity)>, String> {
    // A process created after this cutoff cannot be the one observed by the
    // snapshot entry: reject it instead of combining reused PID/old parent data.
    let cutoff = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos()
        / 100
        + 116_444_736_000_000_000;
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|error| error.to_string())?;
    let _owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).expect("entry size"),
        ..Default::default()
    };
    let mut rows = BTreeMap::new();
    if unsafe { Process32FirstW(handle, &raw mut entry) }.is_err() {
        return Ok(rows);
    }
    loop {
        if let Ok(identity) = ProcessIdentity::open(entry.th32ProcessID)
            && u128::from(identity.created) <= cutoff
        {
            rows.insert(entry.th32ProcessID, (entry.th32ParentProcessID, identity));
        }
        if rows.len() > 4096 {
            return Err("process inventory exceeds bounded capacity".into());
        }
        if unsafe { Process32NextW(handle, &raw mut entry) }.is_err() {
            break;
        }
    }
    Ok(rows)
}

fn root_matches(rows: &BTreeMap<u32, (u32, u64, bool)>) -> Vec<u32> {
    rows.iter()
        .filter_map(|(&pid, &(parent, created, matches))| {
            if !matches {
                return None;
            }
            let mut current = parent;
            let mut child_created = created;
            for _ in 0..rows.len() {
                let Some(&(next, parent_created, parent_matches)) = rows.get(&current) else {
                    break;
                };
                if current == pid || parent_created > child_created {
                    break;
                }
                if parent_matches {
                    return None;
                }
                current = next;
                child_created = parent_created;
            }
            Some(pid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_ancestry_never_collapses_reused_parent_or_unrelated_roots() {
        let rows = BTreeMap::from([(1, (0, 1, true)), (2, (1, 2, false)), (3, (2, 3, true))]);
        assert_eq!(root_matches(&rows), vec![1]);
        let rows = BTreeMap::from([(1, (0, 5, true)), (3, (1, 3, true))]);
        assert_eq!(root_matches(&rows), vec![1, 3]);
        assert_eq!(
            root_matches(&BTreeMap::from([(1, (0, 1, true)), (2, (0, 2, true))])),
            vec![1, 2]
        );
        assert!(root_matches(&BTreeMap::new()).is_empty());
    }
    #[test]
    fn owned_process_identity_is_live_and_exact() {
        let current = ProcessIdentity::open(std::process::id()).unwrap();
        assert!(current.alive());
        assert!(current.creation_time() > 0);
        assert!(Path::new(current.path()).is_absolute());
        assert!(canonical_path("cmd.exe").is_err());
        assert!(ProcessIdentity::open(0).is_err());
    }
}
