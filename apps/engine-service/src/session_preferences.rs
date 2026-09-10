//! Typed audio-session settings. Filesystem replacement and configuration paths
//! belong to persistence; this module validates the session document contract.

use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use tonenari_audio_io::session::{AudioBackendKind, SessionFormat};

use crate::persistence::{StagedWrite, config_directory, stage_json};

const MAX_BYTES: u64 = 4096;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    version: u32,
    backend: AudioBackendKind,
    format: SessionFormat,
}

/// A backend-specific session document at an application configuration path.
#[derive(Debug)]
pub(crate) struct SessionPreferences {
    path: PathBuf,
    backend: AudioBackendKind,
}

impl SessionPreferences {
    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf, backend: AudioBackendKind) -> Self {
        Self { path, backend }
    }

    /// Resolves this backend's settings path without reading or creating a file.
    /// Returns configuration-directory lookup errors.
    pub(crate) fn for_backend(backend: AudioBackendKind) -> io::Result<Self> {
        Ok(Self {
            path: config_directory()?.join("audio-session.json"),
            backend,
        })
    }

    /// Reads a bounded, version/backend-checked document; absence returns `None`.
    /// Returns read, JSON, size-limit or document-contract errors.
    pub(crate) fn load(&self) -> io::Result<Option<SessionFormat>> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session preferences exceed size limit",
            ));
        }
        let value: Preferences = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if value.version != 1 || value.backend != self.backend {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported session preferences version or backend",
            ));
        }
        Ok(Some(value.format))
    }

    /// Stages the session document without accepting it. The caller owns native
    /// format validation and when to commit. Returns file-staging errors.
    pub(crate) fn stage(&self, format: SessionFormat) -> io::Result<StagedWrite> {
        stage_json(
            &self.path,
            &Preferences {
                version: 1,
                backend: self.backend,
                format,
            },
        )
    }
}
