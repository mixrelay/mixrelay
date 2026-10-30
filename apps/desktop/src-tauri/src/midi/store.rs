//! Versioned, atomic desktop preferences; failed writes never publish new bindings.
use std::{
    fs,
    io::{Read as _, Write as _},
    path::PathBuf,
};

use mixrelay_midi_control::{Binding, assign};
use serde::{Deserialize, Serialize};

use super::MidiError;

// Same 1MiB admission limit as the desktop recorder preferences.
const FILE_LIMIT: u64 = 1_048_576;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    bindings: Vec<Binding>,
}

#[derive(Debug)]
pub(super) struct Store {
    path: PathBuf,
    pub(super) bindings: Vec<Binding>,
    pub(super) generation: u64,
}

impl Store {
    pub(super) fn empty(path: PathBuf) -> Self {
        Self {
            path,
            bindings: Vec::new(),
            generation: 0,
        }
    }

    pub(super) fn load(path: PathBuf) -> Result<Self, MidiError> {
        let mut bindings = Vec::new();
        match fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(FILE_LIMIT + 1).read_to_end(&mut bytes)?;
                if bytes.len() > usize::try_from(FILE_LIMIT).unwrap_or_default() {
                    return Err(MidiError::Preferences);
                }
                let saved: Saved = serde_json::from_slice(&bytes)?;
                if saved.version != 1 {
                    return Err(MidiError::Preferences);
                }
                for binding in saved.bindings {
                    assign(&mut bindings, binding)?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(Self {
            path,
            bindings,
            generation: 0,
        })
    }

    pub(super) fn save(&mut self, bindings: Vec<Binding>) -> Result<(), MidiError> {
        for binding in &bindings {
            binding.validate()?;
        }
        let bytes = serde_json::to_vec(&Saved {
            version: 1,
            bindings: bindings.clone(),
        })?;
        if bytes.len() as u64 > FILE_LIMIT {
            return Err(MidiError::Preferences);
        }
        let parent = self.path.parent().ok_or(MidiError::Preferences)?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged.persist(&self.path).map_err(|error| error.error)?;
        if let Err(error) = fs::File::open(parent).and_then(|directory| directory.sync_all()) {
            tracing::warn!(%error, "MIDI preferences directory sync failed after commit");
        }
        self.bindings = bindings;
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
