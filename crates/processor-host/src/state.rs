//! Durable native presets stay outside graph JSON and private IPC configuration.
//! Only the control/worker startup paths perform file I/O; render callbacks never do.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use mixrelay_processor_state::{StateRef, StateStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{HostedProcessorControl, ProcessorControlRegistry, ProcessorStateError};

const MAX_CONFIGURATION_BYTES: u64 = 16 * 1024 * 1024;

pub(super) struct InitialState {
    pub(super) parameters: Value,
    pub(super) native: Option<Vec<u8>>,
}

impl InitialState {
    pub(super) fn resolve(parameters: Value, store: Option<&StateStore>) -> Result<Self, String> {
        let native = reference(&parameters)?
            .map(|reference| {
                store
                    .ok_or_else(|| "processor state store is not configured".to_owned())?
                    .read(&reference)
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        Ok(Self { parameters, native })
    }
}

fn reference(parameters: &Value) -> Result<Option<StateRef>, String> {
    if parameters.get("stateBase64").is_some() {
        return Err("inline processor state is not supported".to_owned());
    }
    parameters
        .get("stateRef")
        .map(|value| serde_json::from_value(value.clone()).map_err(|error| error.to_string()))
        .transpose()
}

impl ProcessorControlRegistry {
    /// Selects durable storage before catalog restoration or state capture.
    /// Existing workers retain their identities; this does not perform audio work.
    pub fn configure_state_store(&self, root: PathBuf) {
        *self
            .state_store
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(StateStore::new(root));
    }

    /// Returns the store shared by capture, worker startup and workspace files.
    /// # Errors
    /// Returns an error if no durable catalog directory has been configured.
    pub fn state_store(&self) -> Result<StateStore, String> {
        self.optional_state_store()
            .ok_or_else(|| "processor state store is not configured".to_owned())
    }

    pub(super) fn optional_state_store(&self) -> Option<StateStore> {
        self.state_store
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn verify_state(&self, parameters: &Value) -> Result<(), String> {
        if let Some(reference) = reference(parameters)? {
            self.state_store()?
                .verify(&reference)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) fn snapshot_control_state(
        &self,
        control: &HostedProcessorControl,
        timeout: Duration,
    ) -> Result<Value, ProcessorStateError> {
        let state = self.capture_control_state(control, timeout)?;
        checkpoint_state(control, &state)?;
        Ok(state)
    }

    pub(super) fn capture_control_state(
        &self,
        control: &HostedProcessorControl,
        timeout: Duration,
    ) -> Result<Value, ProcessorStateError> {
        let store = self
            .state_store()
            .map_err(|_| ProcessorStateError::Storage)?;
        let bytes = control.snapshot_state(timeout)?;
        let reference = store
            .put(&bytes)
            .map_err(|_| ProcessorStateError::Storage)?;
        Ok(serde_json::json!({"stateRef": reference}))
    }
}

// Preparation compares the candidate against this reference. The graph owner's
// reuse transaction restores the old signature if later publication fails;
// durable blob storage alone does not mean the authored graph accepted this preset.
pub(super) fn checkpoint_state(
    control: &HostedProcessorControl,
    state: &Value,
) -> Result<(), ProcessorStateError> {
    if let Some(signature) = control
        .reuse
        .lock()
        .map_err(|_| ProcessorStateError::Unavailable)?
        .as_mut()
    {
        signature.checkpoint(state);
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkerConfiguration {
    sample_rate: u32,
    ports: mixrelay_audio_graph::ProviderPorts,
    parameters: Value,
    state_store: Option<PathBuf>,
}

pub(super) fn write_worker_config(
    path: &Path,
    sample_rate: u32,
    parameters: &Value,
    store: Option<&StateStore>,
    ports: &mixrelay_audio_graph::ProviderPorts,
) -> Result<(), String> {
    ports.validate().map_err(|error| error.to_string())?;
    let body = WorkerConfiguration {
        sample_rate,
        ports: ports.clone(),
        parameters: parameters.clone(),
        state_store: store.map(|store| store.root().to_path_buf()),
    };
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, &body).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())
}

pub(super) struct PreparedConfiguration {
    pub(super) sample_rate: f64,
    pub(super) ports: mixrelay_audio_graph::ProviderPorts,
    pub(super) initial: InitialState,
}

pub(super) fn read_worker_configuration(path: &str) -> Result<PreparedConfiguration, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_CONFIGURATION_BYTES + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_CONFIGURATION_BYTES {
        return Err("processor configuration exceeds size limit".to_owned());
    }
    let config: WorkerConfiguration =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    config.ports.validate().map_err(|error| error.to_string())?;
    if config.sample_rate == 0
        || config
            .state_store
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
    {
        return Err("invalid processor startup configuration".to_owned());
    }
    let store = config.state_store.map(StateStore::new);
    let state = InitialState::resolve(config.parameters, store.as_ref())?;
    Ok(PreparedConfiguration {
        sample_rate: f64::from(config.sample_rate),
        ports: config.ports,
        initial: state,
    })
}

#[cfg(test)]
mod tests;
