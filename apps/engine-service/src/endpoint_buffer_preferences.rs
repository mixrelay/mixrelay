//! Bounded Driver-runtime native client preferences, keyed by stable device identity.
use crate::persistence::{config_directory, stage_json_limited};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

const LIMIT: u64 = 64 * 1024;
const MAX_DEVICES: usize = 256;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    version: u32,
    devices: BTreeMap<String, u32>,
}

/// Settings owned by the audio runtime that creates the native `IOProc`s.
/// No application-local `CoreAudio` readback is stored as a system-wide device setting.
pub struct EndpointBufferPreferences {
    path: PathBuf,
    devices: BTreeMap<String, u32>,
}
impl EndpointBufferPreferences {
    /// Loads a bounded document without modifying the filesystem.
    /// # Errors
    /// Returns path, read or malformed-document errors.
    pub fn load() -> io::Result<Self> {
        Self::at(driver_directory()?.join("endpoint-buffers.json"))
    }

    fn at(path: PathBuf) -> io::Result<Self> {
        let mut result = Self {
            path,
            devices: BTreeMap::new(),
        };
        let file = match File::open(&result.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(result),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "endpoint buffer preferences exceed size limit",
            ));
        }
        let document: Document = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if document.version != 1
            || document.devices.len() > MAX_DEVICES
            || document
                .devices
                .iter()
                .any(|(id, frames)| !valid(id, *frames))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid endpoint buffer preferences",
            ));
        }
        result.devices = document.devices;
        Ok(result)
    }

    /// Returns explicitly retained client intent; absence preserves the OS default.
    #[must_use]
    pub fn frames(&self, id: &str) -> Option<u32> {
        self.devices.get(id).copied()
    }

    /// Persists acknowledged native intent. Memory is updated only after atomic commit.
    /// # Errors
    /// Returns invalid geometry, capacity or filesystem errors.
    pub fn save(&mut self, id: &str, frames: u32) -> io::Result<()> {
        if !valid(id, frames)
            || (self.devices.len() >= MAX_DEVICES && !self.devices.contains_key(id))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid endpoint buffer preference",
            ));
        }
        let mut devices = self.devices.clone();
        devices.insert(id.to_owned(), frames);
        stage_json_limited(
            &self.path,
            &Document {
                version: 1,
                devices: devices.clone(),
            },
            LIMIT,
        )?
        .commit()?;
        self.devices = devices;
        Ok(())
    }
}
fn driver_directory() -> io::Result<PathBuf> {
    let application = config_directory()?;
    // Keep the shared explicit fixture/deployment override. Defaults follow the
    // Driver service so its optional settings removal includes client intent.
    if std::env::var_os("MIXRELAY_CONFIG_DIR").is_some() {
        return Ok(application);
    }
    #[cfg(target_os = "macos")]
    let name = "io.mixrelay.driver";
    #[cfg(not(target_os = "macos"))]
    let name = "mixrelay-driver";
    let parent = application
        .parent()
        .ok_or_else(|| io::Error::other("Driver configuration needs a parent directory"))?;
    Ok(parent.join(name))
}
fn valid(id: &str, frames: u32) -> bool {
    !id.is_empty() && id.len() <= 256 && frames != 0 && frames <= 16_384
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_test_support::FixtureDir;
    #[test]
    fn device_values_are_independent_and_survive_runtime_restart() {
        let dir = FixtureDir::new("endpoint-buffer-preferences").unwrap();
        let path = dir.join("buffers.json");
        let mut prefs = EndpointBufferPreferences::at(path.clone()).unwrap();
        assert_eq!(prefs.frames("a"), None);
        prefs.save("a", 128).unwrap();
        prefs.save("b", 1024).unwrap();
        let restored = EndpointBufferPreferences::at(path).unwrap();
        assert_eq!(restored.frames("a"), Some(128));
        assert_eq!(restored.frames("b"), Some(1024));
    }
    #[test]
    fn corrupt_documents_and_failed_saves_do_not_fabricate_intent() {
        let dir = FixtureDir::new("endpoint-buffer-preferences-invalid").unwrap();
        let path = dir.join("buffers.json");
        std::fs::write(&path, b"{\"version\":2,\"devices\":{}}").unwrap();
        assert!(EndpointBufferPreferences::at(path.clone()).is_err());
        std::fs::remove_file(&path).unwrap();
        let mut prefs = EndpointBufferPreferences::at(path.clone()).unwrap();
        assert!(prefs.save("a", 0).is_err());
        std::fs::create_dir(&path).unwrap();
        assert!(prefs.save("a", 128).is_err());
        assert_eq!(prefs.frames("a"), None);
    }
}
