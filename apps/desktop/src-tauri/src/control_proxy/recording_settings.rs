//! Destination preferences belong to the background controller, outside the audio plan.
//! UI edits and hardware starts share the same serialized store and take preparation.
use std::{collections::BTreeMap, fs, io::Read as _, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager as _;

use super::{ControlProxyClient, Method, recorder};

/// Resolve user-owned defaults once during controller setup, before admitting controllers.
pub(crate) fn initialize(app: &tauri::App) -> Result<(), String> {
    let path = crate::background::data_directory(app.handle())?.join("recording-settings.json");
    let desktop = crate::recording_dialog::default_recording_folder(app)?;
    let store = RecordingStore::load(path, desktop)?;
    let client = app.state::<ControlProxyClient>();
    *client
        .recordings
        .write()
        .map_err(|_| "recording settings lock poisoned")? = store;
    Ok(())
}

/// Settings for the next take. An active take retains its own immutable format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecordingSettings {
    pub folder: String,
    pub sample_rate: Option<u32>,
    pub encoding: RecordingEncoding,
}

/// Explicit UI defaults must not depend on the engine's omitted-value defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RecordingEncoding {
    Pcm16,
    Pcm24,
    Float32,
}

type Entries = BTreeMap<String, BTreeMap<String, RecordingSettings>>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPreferences {
    version: u32,
    entries: Entries,
}

/// The mutex owner serializes durable edits with preparing and submitting a new take.
#[derive(Debug, Default, Clone)]
pub(crate) struct RecordingStore {
    path: Option<PathBuf>,
    default_folder: String,
    entries: Entries,
}

impl RecordingStore {
    pub(crate) fn load(path: PathBuf, default_folder: PathBuf) -> Result<Self, String> {
        let mut entries = BTreeMap::new();
        match fs::File::open(&path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(1_048_577)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 1_048_576 {
                    return Err("recording preferences too large".into());
                }
                let saved: SavedPreferences =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if saved.version != 1 {
                    return Err("unsupported recording preferences version".into());
                }
                for settings in saved.entries.values().flat_map(BTreeMap::values) {
                    settings.validate()?;
                }
                entries = saved.entries;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let default_folder = default_folder
            .into_os_string()
            .into_string()
            .map_err(|_| "recording folder must be valid Unicode")?;
        if !std::path::Path::new(&default_folder).is_absolute() {
            return Err("recording folder must be absolute".into());
        }
        Ok(Self {
            path: Some(path),
            default_folder,
            entries,
        })
    }

    pub(crate) fn get(&self, graph: &str, node: &str) -> RecordingSettings {
        self.entries
            .get(graph)
            .and_then(|nodes| nodes.get(node))
            .cloned()
            .unwrap_or_else(|| RecordingSettings {
                folder: self.default_folder.clone(),
                sample_rate: None,
                encoding: RecordingEncoding::Pcm16,
            })
    }

    fn save(
        &mut self,
        graph: String,
        node: String,
        settings: RecordingSettings,
    ) -> Result<(), String> {
        settings.validate()?;
        if !std::path::Path::new(&settings.folder).is_dir() {
            return Err("select an existing recording folder".into());
        }
        let path = self
            .path
            .clone()
            .ok_or("recording preferences not initialized")?;
        let mut entries = self.entries.clone();
        entries.entry(graph).or_default().insert(node, settings);
        let bytes = serde_json::to_vec(&SavedPreferences {
            version: 1,
            entries: entries.clone(),
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > 1_048_576 {
            return Err("recording preferences too large".into());
        }
        persist(&path, &bytes)?;
        self.entries = entries;
        Ok(())
    }
}

impl RecordingSettings {
    fn validate(&self) -> Result<(), String> {
        if !std::path::Path::new(&self.folder).is_absolute() || self.folder.contains('\0') {
            return Err("recording folder must be absolute".into());
        }
        if self
            .sample_rate
            .is_some_and(|rate| !mixrelay_audio_io::formats::SAMPLE_RATES.contains(&rate))
        {
            return Err("unsupported recording sample rate".into());
        }
        Ok(())
    }

    pub(super) fn start_request(&self) -> Result<serde_json::Value, String> {
        static NEXT_TAKE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        self.validate()?;
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let ticket = NEXT_TAKE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = PathBuf::from(&self.folder).join(format!(
            "MixRelay-{time}-{}-{ticket}.wav",
            std::process::id()
        ));
        Ok(
            serde_json::json!({"action":"record", "path":path, "sampleRate":self.sample_rate, "encoding":self.encoding}),
        )
    }
}

fn persist(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    let directory = path.parent().ok_or("preferences need a parent directory")?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut staged = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    staged.write_all(bytes).map_err(|e| e.to_string())?;
    staged.as_file().sync_all().map_err(|e| e.to_string())?;
    staged.persist(path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if let Err(error) = fs::File::open(directory).and_then(|directory| directory.sync_all()) {
        tracing::warn!(%error, "recording preferences directory sync failed after commit");
    }
    Ok(())
}

/// Read the same next-take settings used by local hardware commands.
#[tauri::command]
pub(crate) async fn recording_settings(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
    node_id: String,
) -> Result<RecordingSettings, String> {
    client.recording_preferences(&graph_id, &node_id)
}

/// Commit a stopped recorder's preferences without modifying its audio graph.
#[tauri::command]
pub(crate) async fn save_recording_settings(
    client: tauri::State<'_, ControlProxyClient>,
    graph_id: String,
    node_id: String,
    settings: RecordingSettings,
    expected_revision: u64,
) -> Result<RecordingSettings, String> {
    let guard = std::sync::Arc::clone(&client.recording_gate)
        .lock_owned()
        .await;
    let snapshot = client
        .request(Method::GET, "/api/v1/controls/snapshot", None)
        .await?;
    if !snapshot.is_success() {
        return Err("recorder settings unavailable".into());
    }
    validate_target(&snapshot.body, &graph_id, &node_id, expected_revision)?;
    let owner = std::sync::Arc::clone(&client.recordings);
    let next = settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        commit_preferences(&owner, guard, (graph_id, node_id), next)
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(settings)
}

fn commit_preferences(
    owner: &std::sync::RwLock<RecordingStore>,
    guard: tokio::sync::OwnedMutexGuard<()>,
    target: (String, String),
    settings: RecordingSettings,
) -> Result<(), String> {
    // Once persistence starts, cancellation cannot separate disk commit from publication.
    // Readers retain the previous snapshot throughout IO; shutdown waits for this guard.
    let _guard = guard;
    let mut store = owner
        .read()
        .map_err(|_| "recording settings lock poisoned")?
        .clone();
    store.save(target.0, target.1, settings)?;
    *owner
        .write()
        .map_err(|_| "recording settings lock poisoned")? = store;
    Ok(())
}

fn validate_target(
    snapshot: &serde_json::Value,
    graph: &str,
    node: &str,
    revision: u64,
) -> Result<(), String> {
    if snapshot["revision"].as_u64() != Some(revision) {
        return Err("graph changed before recording settings were saved".into());
    }
    let target = snapshot["graphs"]
        .as_array()
        .and_then(|graphs| graphs.iter().find(|item| item["id"] == graph))
        .and_then(|graph| graph["nodes"].as_array())
        .and_then(|nodes| nodes.iter().find(|item| item["nodeId"] == node))
        .filter(|node| node["capabilities"]["recorder"] == true)
        .ok_or("recorder no longer exists")?;
    if !matches!(
        target["recorder"]["state"].as_str(),
        Some("stopped" | "failed")
    ) {
        return Err("stop recording before changing recording settings".into());
    }
    Ok(())
}

impl ControlProxyClient {
    pub(super) fn recording_preferences(
        &self,
        graph: &str,
        node: &str,
    ) -> Result<RecordingSettings, String> {
        Ok(self
            .recordings
            .read()
            .map_err(|_| "recording settings lock poisoned")?
            .get(graph, node))
    }
    pub(super) async fn recording_status(
        &self,
        graph: &str,
        node: &str,
    ) -> Result<serde_json::Value, String> {
        let response = self
            .request(Method::GET, &recorder::route(graph, node), None)
            .await?;
        if !response.is_success() {
            return Err(format!("recorder unavailable: {}", response.body));
        }
        Ok(response.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, RecordingStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = RecordingStore::load(
            directory.path().join("preferences.json"),
            directory.path().to_path_buf(),
        )
        .unwrap();
        (directory, store)
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancellation_keeps_commit_owned_and_reads_available_until_shutdown_can_drain() {
        let (directory, store) = fixture();
        let client = ControlProxyClient::default();
        *client.recordings.write().unwrap() = store;
        let mut settings = client.recording_preferences("g", "n").unwrap();
        settings.encoding = RecordingEncoding::Pcm24;
        let next = settings.clone();
        let owner = std::sync::Arc::clone(&client.recordings);
        let guard = std::sync::Arc::clone(&client.recording_gate)
            .lock_owned()
            .await;
        let (ready, started) = tokio::sync::oneshot::channel();
        let (release, proceed) = std::sync::mpsc::channel();
        let caller = tokio::spawn(async move {
            tauri::async_runtime::spawn_blocking(move || {
                ready.send(()).unwrap();
                proceed.recv().unwrap();
                commit_preferences(&owner, guard, ("g".into(), "n".into()), next)
            })
            .await
            .unwrap()
        });
        started.await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        assert_eq!(
            client.recording_preferences("g", "n").unwrap().encoding,
            RecordingEncoding::Pcm16
        );
        assert!(client.recording_gate.try_lock().is_err());
        client.close_admission();
        release.send(()).unwrap();
        client.drain().await.unwrap();
        assert_eq!(client.recording_preferences("g", "n").unwrap(), settings);
        let loaded = RecordingStore::load(
            directory.path().join("preferences.json"),
            directory.path().to_path_buf(),
        )
        .unwrap();
        assert_eq!(loaded.get("g", "n"), settings);
    }

    #[test]
    fn settings_require_the_observed_existing_stopped_target() {
        let mut snapshot = serde_json::json!({"revision":9,"graphs":[{"id":"g","nodes":[{"nodeId":"n","capabilities":{"recorder":true},"recorder":{"state":"stopped"}}]}]});
        assert!(validate_target(&snapshot, "g", "n", 9).is_ok());
        assert!(validate_target(&snapshot, "g", "n", 8).is_err());
        assert!(validate_target(&snapshot, "g", "removed", 9).is_err());
        for state in ["recording", "paused"] {
            snapshot["graphs"][0]["nodes"][0]["recorder"]["state"] = serde_json::json!(state);
            assert!(validate_target(&snapshot, "g", "n", 9).is_err());
        }
    }

    #[test]
    fn shared_settings_survive_restart_and_remain_target_specific() {
        let (directory, mut store) = fixture();
        let mut settings = store.get("graph", "recorder");
        assert_eq!(settings.encoding, RecordingEncoding::Pcm16);
        assert_eq!(settings.sample_rate, None);
        settings.sample_rate = Some(96_000);
        settings.encoding = RecordingEncoding::Pcm24;
        store
            .save("graph".into(), "recorder".into(), settings.clone())
            .unwrap();
        let restored = RecordingStore::load(
            directory.path().join("preferences.json"),
            directory.path().to_path_buf(),
        )
        .unwrap();
        assert_eq!(restored.get("graph", "recorder"), settings);
        assert_eq!(
            restored.get("other", "recorder").encoding,
            RecordingEncoding::Pcm16
        );
        assert_eq!(restored.get("graph", "other").sample_rate, None);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn rejected_preferences_do_not_change_memory_or_durable_state() {
        let (directory, mut store) = fixture();
        let settings = store.get("g", "n");
        store
            .save("g".into(), "n".into(), settings.clone())
            .unwrap();
        let before = fs::read(directory.path().join("preferences.json")).unwrap();
        let mut invalid = settings.clone();
        invalid.sample_rate = Some(123);
        assert!(store.save("g".into(), "n".into(), invalid).is_err());
        assert_eq!(store.get("g", "n"), settings);
        assert_eq!(
            fs::read(directory.path().join("preferences.json")).unwrap(),
            before
        );
    }

    #[test]
    fn failed_atomic_replace_keeps_previous_in_memory_preferences() {
        let (directory, mut store) = fixture();
        fs::create_dir(directory.path().join("preferences.json")).unwrap();
        let before = store.get("g", "n");
        let mut settings = before.clone();
        settings.encoding = RecordingEncoding::Float32;
        assert!(store.save("g".into(), "n".into(), settings).is_err());
        assert_eq!(store.get("g", "n"), before);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn new_take_names_are_unique_uncreated_and_use_canonical_format() {
        let (_directory, store) = fixture();
        let settings = store.get("g", "n");
        let first = settings.start_request().unwrap();
        let second = settings.start_request().unwrap();
        assert_ne!(first["path"], second["path"]);
        assert!(!std::path::Path::new(first["path"].as_str().unwrap()).exists());
        assert_eq!(first["encoding"], "pcm16");
        assert_eq!(first["sampleRate"], serde_json::Value::Null);
        assert_eq!(
            PathBuf::from(first["path"].as_str().unwrap())
                .parent()
                .unwrap(),
            std::path::Path::new(&settings.folder)
        );
    }
}
