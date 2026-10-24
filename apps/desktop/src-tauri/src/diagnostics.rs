//! Explicit, bounded support export. No recorder, timer, retained dump, or upload.
use mixrelay_driver_control::DriverSnapshot;
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager as _;

mod editor_context;
use editor_context::EditorContext;

const REPORT_LIMIT: usize = 256 * 1024;
const DEVICE_LIMIT: usize = 64;
static EXPORTING: AtomicBool = AtomicBool::new(false);

struct ExportGuard(tauri::AppHandle);
impl Drop for ExportGuard {
    fn drop(&mut self) {
        EXPORTING.store(false, Ordering::Release);
        if let Some(menu) = self.0.try_state::<crate::status_menu::StatusMenu>() {
            menu.refresh_diagnostics();
        }
    }
}

pub(crate) fn is_exporting() -> bool {
    EXPORTING.load(Ordering::Acquire)
}

pub(crate) fn label(language: &str) -> &'static str {
    LABELS[crate::native_text::resolve_locale(language).index()].0
}

const LABELS: [(&str, &str); 11] = [
    (
        "Save Diagnostic Report…",
        "Could not save the diagnostic report. Check the destination and try again.",
    ),
    (
        "진단 리포트 저장…",
        "진단 리포트를 저장하지 못했습니다. 저장 위치를 확인하고 다시 시도하십시오.",
    ),
    (
        "診断レポートを保存…",
        "診断レポートを保存できませんでした。保存先を確認して再試行してください。",
    ),
    ("保存诊断报告…", "无法保存诊断报告。请检查保存位置并重试。"),
    ("儲存診斷報告…", "無法儲存診斷報告。請檢查儲存位置並重試。"),
    (
        "Enregistrer un rapport de diagnostic…",
        "Impossible d’enregistrer le rapport de diagnostic. Vérifiez la destination et réessayez.",
    ),
    (
        "Diagnosebericht speichern…",
        "Der Diagnosebericht konnte nicht gespeichert werden. Prüfen Sie den Speicherort und versuchen Sie es erneut.",
    ),
    (
        "Guardar informe de diagnóstico…",
        "No se pudo guardar el informe de diagnóstico. Compruebe el destino e inténtelo de nuevo.",
    ),
    (
        "Salvar relatório de diagnóstico…",
        "Não foi possível salvar o relatório de diagnóstico. Verifique o destino e tente novamente.",
    ),
    (
        "Salva rapporto diagnostico…",
        "Impossibile salvare il rapporto diagnostico. Controlla la destinazione e riprova.",
    ),
    (
        "Сохранить диагностический отчёт…",
        "Не удалось сохранить диагностический отчёт. Проверьте место сохранения и повторите попытку.",
    ),
];

/// A manual save chooser is the only admission path. Cancellation creates nothing.
/// Saves the support report the user asked for from the diagnostics window, adding the editor's
/// current problem codes and scan-failure counts.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands receive owned arguments; the app handle is borrowed for the export"
)]
pub(crate) fn diagnostics_save_report(app: tauri::AppHandle, context: EditorContext) {
    export(&app, Some(context));
}

pub(crate) fn export(app: &tauri::AppHandle, editor: Option<EditorContext>) {
    if EXPORTING.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(menu) = app.try_state::<crate::status_menu::StatusMenu>() {
        menu.refresh_diagnostics();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = ExportGuard(app.clone());
        let locale = crate::native_text::current_locale();
        let text = LABELS[locale.index()];
        let mut chooser = rfd::AsyncFileDialog::new()
            .set_title(text.0)
            .set_file_name("dump.json")
            .add_filter("JSON", &["json"]);
        if let Some(window) = app.get_webview_window("main") {
            chooser = chooser.set_parent(&window);
        }
        let Some(file) = chooser.save_file().await else {
            return;
        };
        let driver = tokio::time::timeout(Duration::from_secs(4), async {
            crate::driver_control::session_client()?.snapshot().await
        })
        .await
        .ok()
        .and_then(Result::ok);
        let runtime = tokio::time::timeout(
            Duration::from_secs(4),
            crate::background::dispatch(&app, "control_get_status", Value::Null),
        )
        .await
        .ok()
        .and_then(Result::ok);
        let destination = file.path().to_path_buf();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let report = report(
                driver.as_ref(),
                runtime.as_ref(),
                editor.as_ref(),
                &os_version(),
            );
            save(&destination, &report)
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            rfd::AsyncMessageDialog::new()
                .set_title("MixRelay")
                .set_description(text.1)
                .set_level(rfd::MessageLevel::Error)
                .set_buttons(rfd::MessageButtons::OkCustom(
                    crate::native_text::dialogs::text(
                        locale,
                        crate::native_text::dialogs::Text::Ok,
                    )
                    .into(),
                ))
                .show()
                .await;
        }
    });
}

fn os_version() -> String {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("/usr/bin/sw_vers");
    #[cfg(target_os = "macos")]
    command.arg("-productVersion");
    #[cfg(target_os = "linux")]
    let mut command = std::process::Command::new("/usr/bin/uname");
    #[cfg(target_os = "linux")]
    command.arg("-r");
    command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .filter(|value| {
            value.len() <= 128
                && value
                    .trim()
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".-_+".contains(&byte))
        })
        .map_or_else(|| "unavailable".into(), |value| value.trim().into())
}

fn report(
    driver: Option<&DriverSnapshot>,
    runtime: Option<&Value>,
    editor: Option<&EditorContext>,
    os_version: &str,
) -> Value {
    let devices = driver.map(|snapshot| {
        snapshot
            .settings
            .devices
            .iter()
            .take(DEVICE_LIMIT)
            .enumerate()
            .map(|(index, device)| {
                json!({
                    "index": index + 1,
                    "render": device.native_formats.render.current,
                    "capture": device.native_formats.capture.current,
                    "observedSampleRate": device.observed_sample_rate,
                    "renderChannels": device.spec.render.map(|stream| stream.channels),
                    "captureChannels": device.spec.capture.map(|stream| stream.channels),
                })
            })
            .collect::<Vec<_>>()
    });
    let driver_version = driver
        .and_then(|snapshot| snapshot.installed_version.as_deref())
        .filter(|version| version.len() <= 64)
        .and_then(|version| semver::Version::parse(version).ok())
        .map(|version| format!("{}.{}.{}", version.major, version.minor, version.patch));
    json!({
        "schemaVersion": 1,
        "product": "MixRelay Driver",
        "buildVersion": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "osVersion": os_version,
        "createdUnixSeconds": SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|time| time.as_secs()),
        "collection": "manual_snapshot",
        "driver": {
            "available": driver.is_some(),
            "version": driver_version,
            "state": driver.map(|snapshot| snapshot.state),
            "revision": driver.map(|snapshot| snapshot.revision),
            "protocolVersion": driver.map(|snapshot| snapshot.protocol_version),
            "persistenceFailed": driver.map(|snapshot| snapshot.persistence_error.is_some()),
            "deviceCount": driver.map(|snapshot| snapshot.settings.devices.len()),
            "devicesTruncated": driver.is_some_and(|snapshot| snapshot.settings.devices.len() > DEVICE_LIMIT),
            "devices": devices,
        },
        "runtime": runtime_summary(runtime),
        "editor": editor.map(EditorContext::to_report),
        "exclusions": ["audio", "memory", "logs", "credentials", "license", "user_paths", "device_names", "device_ids", "graphs", "application_list"],
    })
}

// Copy facts through a positive schema, never redact a full arbitrary response afterwards.
fn runtime_summary(response: Option<&Value>) -> Value {
    let body = response
        .filter(|value| value["status"] == 200)
        .map(|value| &value["body"]);
    let mut summary = json!({"available": body.is_some()});
    let Some(body) = body else {
        return summary;
    };
    for key in [
        "protocolVersion",
        "sampleRate",
        "bufferFrames",
        "underruns",
        "overruns",
    ] {
        if let Some(value) = body[key].as_u64() {
            summary[key] = json!(value);
        }
    }
    let session = &body["audioSession"];
    for key in ["sourceSamples", "inputSamples", "outputSamples", "blocks"] {
        if let Some(value) = body["sampleFaults"][key].as_u64() {
            summary["sampleFaults"][key] = json!(value);
        }
    }
    for key in [
        "discontinuities",
        "observedDeviceSampleRate",
        "observedDeviceBufferFrames",
        "transportLatencyFrames",
    ] {
        if let Some(value) = session[key].as_u64() {
            summary["audioSession"][key] = json!(value);
        }
    }
    for key in ["sampleRate", "blockFrames"] {
        if let Some(value) = session["format"][key].as_u64() {
            summary["audioSession"]["format"][key] = json!(value);
        }
    }
    if let Ok(state) = serde_json::from_value::<mixrelay_audio_io::process::ProcessingState>(
        session["processingState"].clone(),
    ) {
        summary["audioSession"]["processingState"] = json!(state);
    }
    if let Ok(issue) = serde_json::from_value::<Option<mixrelay_audio_io::session::SessionIssue>>(
        session["issue"].clone(),
    ) {
        summary["audioSession"]["issue"] = json!(issue);
    }
    for key in ["enabled", "transitioning"] {
        if let Some(value) = body["enginePower"][key].as_bool() {
            summary["enginePower"][key] = json!(value);
        }
    }
    summary
}

/// One private same-directory temporary file, cleaned on failure and atomically consumed on save.
fn save(destination: &Path, report: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(report)?;
    if bytes.len() >= REPORT_LIMIT {
        return Err(std::io::Error::other(
            "diagnostic report exceeds its fixed limit",
        ));
    }
    let parent = destination
        .parent()
        .filter(|parent| parent.is_dir())
        .ok_or_else(|| std::io::Error::other("diagnostic destination has no parent"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(destination).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_is_useful_when_runtime_is_unavailable_and_never_embeds_errors() {
        let value = report(
            None,
            Some(&json!({"status":503,"body":{"reason":"SECRET"}})),
            None,
            "27.0.1",
        );
        assert_eq!(value["runtime"]["available"], false);
        assert_eq!(value["driver"]["available"], false);
        assert!(!value.to_string().contains("SECRET"));
    }

    #[test]
    fn positive_schema_keeps_counters_but_excludes_private_and_unknown_payloads() {
        let response = json!({"status":200,"body":{
            "underruns":8,"sampleRate":48000,"license":"SECRET", "graph":{"name":"SECRET"},
            "sampleFaults":{"sourceSamples":2,"inputSamples":3,"outputSamples":4,"blocks":1,"samples":["SECRET"],"nodeId":"SECRET"},
            "audioSession":{"discontinuities":3,"processingState":"SECRET", "issue":{"path":"SECRET"},
                "virtualEndpointSettings":{"devices":[{"name":"SECRET"}]},"format":{"sampleRate":48000,"blockFrames":128,"path":"SECRET"}}
        }});
        let value = report(None, Some(&response), None, "27.0.1");
        assert_eq!(value["runtime"]["underruns"], 8);
        assert_eq!(
            value["runtime"]["sampleFaults"],
            json!({"sourceSamples":2,"inputSamples":3,"outputSamples":4,"blocks":1})
        );
        assert_eq!(value["runtime"]["audioSession"]["discontinuities"], 3);
        assert_eq!(
            value["runtime"]["audioSession"]["format"]["blockFrames"],
            128
        );
        assert!(!value.to_string().contains("SECRET"));
    }

    #[test]
    fn editor_context_is_included_only_from_the_diagnostics_window() {
        let context: EditorContext = serde_json::from_value(json!({
            "problems": ["graph_apply_timeout"],
            "processorScan": [{"format": "clap", "stage": "scan", "count": 2}],
        }))
        .unwrap();
        let window = report(None, None, Some(&context), "27.0.1");
        assert_eq!(window["editor"]["problems"], json!(["graph_apply_timeout"]));
        assert_eq!(window["editor"]["processorScan"][0]["count"], 2);
        assert_eq!(report(None, None, None, "27.0.1")["editor"], Value::Null);
    }

    #[test]
    fn repeated_explicit_save_replaces_one_file_and_failure_leaves_no_temporary_dump() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("report.json");
        save(&path, &json!({"sequence":1})).unwrap();
        save(&path, &json!({"sequence":2})).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap()["sequence"],
            2
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(save(directory.path(), &json!({})).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(save(&path, &json!("x".repeat(REPORT_LIMIT))).is_err());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
